#[cfg(target_os = "linux")]
mod linux {
    use std::env;
    use std::error::Error;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use virtualfacility::{apply_plan, run_in_network_namespace, Topology};

    const SERVER_PORT: u16 = 18080;

    #[test]
    fn rust_server_and_client_run_inside_pod_namespaces_when_requested(
    ) -> Result<(), Box<dyn Error>> {
        if env::var("VIRTUALFACILITY_IN_PROCESS_SMOKE").as_deref() != Ok("1") {
            eprintln!(
                "skipping in-process smoke; set VIRTUALFACILITY_IN_PROCESS_SMOKE=1 to run it"
            );
            return Ok(());
        }

        let topology = Topology::builder("in-process")
            .add_node("default-node")
            .add_pod("server", "default-node")
            .add_pod("client", "default-node")
            .build()?;
        let setup = topology.setup_plan();
        let cleanup = topology.cleanup_plan();
        let server_ip = topology.resolve("server").expect("server pod exists");
        let server_ns_path = format!("/run/netns/{}", topology.pod_namespace("server")?);
        let client_ns_path = format!("/run/netns/{}", topology.pod_namespace("client")?);

        if let Err(err) = apply_plan(&setup) {
            let _ = apply_plan(&cleanup);
            return Err(Box::new(err));
        }

        let (ready_tx, ready_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            run_in_network_namespace(server_ns_path, move || {
                let listener = TcpListener::bind((server_ip, SERVER_PORT)).map_err(|err| {
                    virtualfacility::FacilityError::NamespaceSyscall {
                        syscall: "TcpListener::bind",
                        detail: err.to_string(),
                    }
                })?;
                ready_tx.send(()).map_err(|err| {
                    virtualfacility::FacilityError::NamespaceSyscall {
                        syscall: "mpsc::Sender::send",
                        detail: err.to_string(),
                    }
                })?;
                let (mut stream, _) = listener.accept().map_err(|err| {
                    virtualfacility::FacilityError::NamespaceSyscall {
                        syscall: "TcpListener::accept",
                        detail: err.to_string(),
                    }
                })?;
                let mut request = [0_u8; 4];
                stream.read_exact(&mut request).map_err(|err| {
                    virtualfacility::FacilityError::NamespaceSyscall {
                        syscall: "TcpStream::read_exact",
                        detail: err.to_string(),
                    }
                })?;
                if &request != b"ping" {
                    return Err(virtualfacility::FacilityError::NamespaceSyscall {
                        syscall: "workload assertion",
                        detail: format!("unexpected request: {request:?}"),
                    });
                }
                stream.write_all(b"pong").map_err(|err| {
                    virtualfacility::FacilityError::NamespaceSyscall {
                        syscall: "TcpStream::write_all",
                        detail: err.to_string(),
                    }
                })
            })
        });

        ready_rx.recv_timeout(Duration::from_secs(5))?;
        let client = run_in_network_namespace(client_ns_path, move || {
            let mut stream = TcpStream::connect((server_ip, SERVER_PORT)).map_err(|err| {
                virtualfacility::FacilityError::NamespaceSyscall {
                    syscall: "TcpStream::connect",
                    detail: err.to_string(),
                }
            })?;
            stream.write_all(b"ping").map_err(|err| {
                virtualfacility::FacilityError::NamespaceSyscall {
                    syscall: "TcpStream::write_all",
                    detail: err.to_string(),
                }
            })?;
            let mut response = [0_u8; 4];
            stream.read_exact(&mut response).map_err(|err| {
                virtualfacility::FacilityError::NamespaceSyscall {
                    syscall: "TcpStream::read_exact",
                    detail: err.to_string(),
                }
            })?;
            if &response != b"pong" {
                return Err(virtualfacility::FacilityError::NamespaceSyscall {
                    syscall: "workload assertion",
                    detail: format!("unexpected response: {response:?}"),
                });
            }
            Ok(())
        });

        let server_result = server.join().map_err(|_| "server thread panicked")?;
        let cleanup_result = apply_plan(&cleanup);
        client?;
        server_result?;
        cleanup_result?;
        Ok(())
    }
}
