use std::env;
use std::error::Error;

use virtualfacility::{apply_plan, Topology};

#[test]
fn live_smoke_client_reaches_server_when_requested() -> Result<(), Box<dyn Error>> {
    if env::var("VIRTUALFACILITY_LIVE_SMOKE").as_deref() != Ok("1") {
        eprintln!("skipping live smoke; set VIRTUALFACILITY_LIVE_SMOKE=1 to run it");
        return Ok(());
    }

    let topology = Topology::smoke()?;
    let setup = topology.setup_plan();
    let ping = topology.ping_plan("client", "server")?;
    let cleanup = topology.cleanup_plan();

    if let Err(err) = apply_plan(&setup) {
        let _ = apply_plan(&cleanup);
        return Err(Box::new(err));
    }

    let ping_result = apply_plan(&ping);
    let cleanup_result = apply_plan(&cleanup);
    ping_result?;
    cleanup_result?;
    Ok(())
}
