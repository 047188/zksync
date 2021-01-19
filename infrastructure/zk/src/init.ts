import { Command } from 'commander';
import * as utils from './utils';

import * as db from './db/db';
import * as server from './server';
import * as contract from './contract';
import * as run from './run/run';
import * as env from './env';
import { up } from './up';

export async function init() {
    await createVolumes();
    if (!process.env.CI) {
        await checkEnv();
        await env.gitHooks();
        await up();
    }
    await run.yarn();
    await run.plonkSetup();
    await run.verifyKeys.unpack();
    await db.setup();
    await contract.build();
    await run.deployERC20('dev');
    await run.deployEIP1271();
    await server.genesis();
    await contract.redeploy();
}

async function createVolumes() {
    await utils.exec('mkdir -p $ZKSYNC_HOME/volumes/geth');
    await utils.exec('mkdir -p $ZKSYNC_HOME/volumes/postgres');
    await utils.exec('mkdir -p $ZKSYNC_HOME/volumes/tesseracts');
}

async function checkEnv() {
    const tools = ['node', 'yarn', 'docker', 'docker-compose', 'cargo', 'psql', 'pg_isready', 'diesel', 'solc'];
    for (const tool of tools) {
        await utils.exec(`which ${tool}`);
    }
    await utils.exec('cargo sqlx --version');
    const { stdout: version } = await utils.exec('node --version');
    // Node v.14.14 is required because
    // the `fs.rmSync` function was added in v14.14.0
    if ('v14.14' >= version) {
        throw new Error('Error, node.js version 14.14.0 or higher is required');
    }
}

export const command = new Command('init')
    .description('perform zksync network initialization for development')
    .action(init);
