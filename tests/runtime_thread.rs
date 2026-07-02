#[cfg(target_os = "linux")]
mod linux {
    use std::env;
    use std::error::Error;

    use virtualfacility::{
        apply_plan, run_in_network_namespace, FacilityError, Result as FacilityResult, Topology,
    };

    #[test]
    fn tokio_runtime_runs_inside_namespace_thread_when_requested() -> Result<(), Box<dyn Error>> {
        if env::var("VIRTUALFACILITY_RUNTIME_SMOKE").as_deref() != Ok("1") {
            eprintln!("skipping runtime smoke; set VIRTUALFACILITY_RUNTIME_SMOKE=1 to run it");
            return Ok(());
        }

        let topology = Topology::builder("runtime-smoke")
            .add_node("default-node")
            .add_pod("runtime", "default-node")
            .build()?;
        let setup = topology.setup_plan();
        let cleanup = topology.cleanup_plan();
        let runtime_ns = topology.pod_namespace("runtime")?;
        let runtime_ns_path = format!("/run/netns/{runtime_ns}");

        if let Err(err) = apply_plan(&setup) {
            let _ = apply_plan(&cleanup);
            return Err(Box::new(err));
        }

        let runtime_result = run_in_network_namespace(runtime_ns_path, || {
            build_current_thread_runtime()?.block_on(async { Ok::<_, FacilityError>(()) })
        });
        let cleanup_result = apply_plan(&cleanup);
        runtime_result?;
        cleanup_result?;
        Ok(())
    }

    fn build_current_thread_runtime() -> FacilityResult<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| FacilityError::NamespaceSyscall {
                syscall: "tokio::runtime::Builder::build",
                detail: err.to_string(),
            })
    }
}
