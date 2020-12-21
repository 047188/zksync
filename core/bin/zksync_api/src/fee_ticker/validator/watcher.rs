use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use zksync_types::{Address, Token};

use crate::fee_ticker::ticker_api::REQUEST_TIMEOUT;
use bigdecimal::BigDecimal;

#[async_trait::async_trait]
pub trait TokenWatcher {
    async fn get_token_market_volume(&mut self, token: &Token) -> anyhow::Result<BigDecimal>;
}

/// Watcher for Uniswap protocol
/// https://thegraph.com/explorer/subgraph/uniswap/uniswap-v2
#[derive(Clone)]
pub struct UniswapTokenWatcher {
    client: reqwest::Client,
    addr: String,
    cache: Arc<Mutex<HashMap<Address, BigDecimal>>>,
}

impl UniswapTokenWatcher {
    pub fn new(addr: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            addr,
            cache: Default::default(),
        }
    }
    async fn get_market_volume(&mut self, address: Address) -> anyhow::Result<BigDecimal> {
        // Uniswap has graphql API, using full graphql client for one query is overkill for current task
        let query = format!("{{token(id: \"{:?}\"){{tradeVolumeUSD}}}}", address);

        let request = self.client.post(&self.addr).json(&serde_json::json!({
            "query": query.clone(),
        }));
        let api_request_future = tokio::time::timeout(REQUEST_TIMEOUT, request.send());

        let response: GraphqlResponse = api_request_future
            .await
            .map_err(|_| anyhow::format_err!("Uniswap API request timeout"))?
            .map_err(|err| anyhow::format_err!("Uniswap API request failed: {}", err))?
            .json::<GraphqlResponse>()
            .await?;

        Ok(response.data.token.trade_volume_usd.parse()?)
    }
    async fn update_historical_amount(&mut self, address: Address, amount: BigDecimal) {
        let mut cache = self.cache.lock().await;
        cache.insert(address, amount);
    }
    async fn get_historical_amount(&mut self, address: Address) -> Option<BigDecimal> {
        let cache = self.cache.lock().await;
        cache.get(&address).cloned()
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct GraphqlResponse {
    data: GraphqlTokenResponse,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct GraphqlTokenResponse {
    token: TokenResponse,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct TokenResponse {
    #[serde(rename = "tradeVolumeUSD")]
    trade_volume_usd: String,
}

#[async_trait::async_trait]
impl TokenWatcher for UniswapTokenWatcher {
    async fn get_token_market_volume(&mut self, token: &Token) -> anyhow::Result<BigDecimal> {
        match self.get_market_volume(token.address).await {
            Ok(amount) => {
                self.update_historical_amount(token.address, amount.clone())
                    .await;
                return Ok(amount);
            }
            Err(err) => {
                vlog::error!("Error in api: {:?}", err);
            }
        }

        if let Some(amount) = self.get_historical_amount(token.address).await {
            return Ok(amount);
        };
        anyhow::bail!("Token amount api is not available right now.")
    }
}
