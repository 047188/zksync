use std::convert::TryInto;

use rand::{rngs::SmallRng, thread_rng, RngCore, SeedableRng};

use zksync::web3::signing::keccak256;
use zksync_types::H256;

// SmallRng seed type is [u8; 16].
const SEED_SIZE: usize = 16;

#[derive(Debug)]
pub struct LoadtestRng {
    pub seed: [u8; SEED_SIZE],
    rng: SmallRng,
}

impl LoadtestRng {
    pub fn new_generic(seed: Option<[u8; SEED_SIZE]>) -> Self {
        let seed: [u8; SEED_SIZE] = seed.unwrap_or_else(|| {
            let rng = &mut thread_rng();
            let mut output = [0u8; SEED_SIZE];
            rng.fill_bytes(&mut output);

            output
        });

        let rng = SmallRng::from_seed(seed);

        Self { seed, rng }
    }

    pub fn derive(&self, eth_pk: H256) -> Self {
        // We chain the current seed bytes and the Ethereum private key together,
        // and then calculate the hash of this data.
        // This way we obtain a derived seed, unique for each wallet, which will result in
        // an uniques set of operations for each account.
        let input_bytes: Vec<u8> = self
            .seed
            .iter()
            .flat_map(|val| val.to_be_bytes().to_vec())
            .chain(eth_pk.as_bytes().iter().copied())
            .collect();
        let data_hash = keccak256(input_bytes.as_ref());
        let new_seed = data_hash[..SEED_SIZE].try_into().unwrap();

        let rng = SmallRng::from_seed(new_seed);
        Self {
            seed: new_seed,
            rng,
        }
    }
}

impl RngCore for LoadtestRng {
    fn next_u32(&mut self) -> u32 {
        self.rng.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.rng.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.rng.fill_bytes(dest)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.rng.try_fill_bytes(dest)
    }
}
