use std::env;
use std::fs;
use std::process::{self, Command as ProcessCommand};

use virtualfacility::{
    apply_plan, apply_rootless_bootstrap_plan, support_report, CommandPlan, CommandSpec, Result,
    RootlessBootstrap, Topology,
};

const CONTEXT_FILE: &str = ".virtualfacility-context";

#[derive(Debug, Clone)]
struct FacilityContext {
    name: String,
    bridge: String,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let command_index = first_command_index(&args).unwrap_or(0);
    let command = args
        .get(command_index)
        .map(String::as_str)
        .unwrap_or("plan");
    let command_args = if command_index < args.len() {
        let mut out = Vec::new();
        out.extend_from_slice(&args[..command_index]);
        out.extend_from_slice(&args[command_index + 1..]);
        out
    } else {
        Vec::new()
    };
    match command {
        "plan" => {
            let topology = topology_from_args(&command_args)?;
            println!("{}", topology.render_summary());
            println!("\n{}", topology.setup_plan().render_shell());
        }
        "workloads" => {
            let topology = topology_from_args(&command_args)?;
            print_plan(topology.workload_plan()?);
        }
        "cleanup" => {
            let topology = topology_from_args(&command_args)?;
            let plan = topology.cleanup_plan();
            if has_confirmed_apply(&args) {
                apply_plan(&plan)?;
            } else {
                print_plan(plan);
            }
        }
        "check" => {
            println!("{}", support_report().render());
        }
        "bootstrap" => {
            let plan = RootlessBootstrap::new("/tmp/virtualfacility")
                .with_standard_mounts()
                .bootstrap_plan(CommandSpec::new("cargo", ["test"]))?;
            if has_confirmed_apply(&args) {
                apply_rootless_bootstrap_plan(&plan)?;
            } else {
                print_plan(plan);
            }
        }
        "smoke" => {
            run_smoke(has_confirmed_apply(&args), &command_args)?;
        }
        "up" => {
            run_up(&command_args)?;
        }
        "down" => {
            run_down(&command_args)?;
        }
        "status" => {
            run_status(&command_args)?;
        }
        "ping" => {
            run_ping(&command_args)?;
        }
        "exec" => {
            run_exec(&command_args)?;
        }
        "create" => {
            run_create(&command_args)?;
        }
        "delete" => {
            run_delete(&command_args)?;
        }
        "use" => {
            run_use(&command_args)?;
        }
        "current" => {
            run_current()?;
        }
        "apply" => {
            if !has_confirmed_apply(&args) {
                eprintln!("{}", usage());
                process::exit(2);
            }
            let topology = topology_from_args(&command_args)?;
            apply_plan(&topology.setup_plan())?;
        }
        "-h" | "--help" | "help" => {
            println!("{}", usage());
        }
        other => {
            eprintln!("unknown command `{other}`");
            eprintln!("{}", usage());
            process::exit(2);
        }
    }
    Ok(())
}

fn print_plan(plan: CommandPlan) {
    println!("{}", plan.render_shell());
}

fn has_confirmed_apply(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--i-understand")
}

fn default_context() -> FacilityContext {
    FacilityContext {
        name: "smoke".to_string(),
        bridge: "vf-br0".to_string(),
    }
}

fn default_bridge_for_name(name: &str) -> String {
    if name == "smoke" {
        return "vf-br0".to_string();
    }
    let suffix = name.chars().take(36).collect::<String>();
    format!("vf-{suffix}")
}

fn read_context() -> FacilityContext {
    read_saved_context().unwrap_or_else(default_context)
}

fn read_saved_context() -> Option<FacilityContext> {
    let data = fs::read_to_string(CONTEXT_FILE).ok()?;
    let mut context = default_context();
    for line in data.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "name" if !value.is_empty() => context.name = value.to_string(),
            "bridge" if !value.is_empty() => context.bridge = value.to_string(),
            _ => {}
        }
    }
    Some(context)
}

fn write_context(context: &FacilityContext) -> Result<()> {
    let data = format!("name={}\nbridge={}\n", context.name, context.bridge);
    fs::write(CONTEXT_FILE, data).map_err(|err| virtualfacility::FacilityError::CommandFailed {
        command: format!("write {CONTEXT_FILE}"),
        code: None,
        stderr: err.to_string(),
    })
}

fn first_command_index(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" | "--bridge" => {
                i += 2;
            }
            "--i-understand" => {
                i += 1;
            }
            _ => return Some(i),
        }
    }
    None
}

fn topology_from_args(args: &[String]) -> Result<Topology> {
    let context = read_context();
    let mut facility_name = context.name;
    let mut bridge_name = context.bridge;
    let mut name_was_set = false;
    let mut bridge_was_set = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("--name requires a value");
                    process::exit(2);
                };
                facility_name = value.clone();
                name_was_set = true;
                i += 2;
            }
            "--bridge" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("--bridge requires a value");
                    process::exit(2);
                };
                bridge_name = value.clone();
                bridge_was_set = true;
                i += 2;
            }
            "--i-understand" => {
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    if name_was_set && !bridge_was_set {
        bridge_name = default_bridge_for_name(&facility_name);
    }

    let server_url = "http://10.244.2.2:8080".to_string();
    Topology::builder(facility_name)
        .bridge_name(bridge_name)
        .add_node("default-node")
        .add_workload_pod("proxy", "default-node", ["proxy", "run"])
        .add_workload_pod(
            "server",
            "default-node",
            ["python3", "-m", "http.server", "8080"],
        )
        .add_workload_pod("client", "default-node", ["curl".to_string(), server_url])
        .build()
}

fn explicit_bridge(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--bridge" {
            return args.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

fn explicit_name(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--name" {
            return args.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

fn facility_from_bridge(bridge: &str) -> String {
    if let Some(name) = bridge.strip_prefix("vf-") {
        return name.to_string();
    }
    if let Some(name) = bridge.strip_prefix("vf") {
        if !name.is_empty() {
            return name.to_string();
        }
    }
    bridge.to_string()
}

fn args_with_context(name: &str, bridge: &str) -> Vec<String> {
    vec![
        "--name".to_string(),
        name.to_string(),
        "--bridge".to_string(),
        bridge.to_string(),
    ]
}

fn bridge_context_or_exit(args: &[String], op_args: &[String], action: &str) -> FacilityContext {
    if let Some(bridge) = op_args.get(1) {
        return FacilityContext {
            name: explicit_name(args).unwrap_or_else(|| facility_from_bridge(bridge)),
            bridge: bridge.clone(),
        };
    }
    if let Some(bridge) = explicit_bridge(args) {
        return FacilityContext {
            name: explicit_name(args).unwrap_or_else(|| facility_from_bridge(&bridge)),
            bridge,
        };
    }

    let suggested = explicit_name(args)
        .map(|name| default_bridge_for_name(&name))
        .or_else(|| read_saved_context().map(|context| context.bridge))
        .unwrap_or_else(|| default_context().bridge);
    eprintln!("bridge name is required.");
    eprintln!("suggested: {suggested}");
    eprintln!("example: cargo run -- {action} bridge {suggested}");
    if suggested != "vf-lab1" {
        eprintln!("example: cargo run -- {action} bridge vf-lab1");
    }
    process::exit(2);
}

fn operation_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" | "--bridge" => {
                i += 2;
            }
            "--i-understand" => {
                i += 1;
            }
            _ => {
                out.push(args[i].clone());
                i += 1;
            }
        }
    }
    out
}

fn run_up(args: &[String]) -> Result<()> {
    let topology = topology_from_args(args)?;
    println!("creating topology: 1 node, 3 pods");
    println!("name={} bridge={}", topology.name(), topology.bridge_name());
    apply_plan(&topology.setup_plan())?;
    println!("UP: topology is running.");
    println!("try: cargo run -- status");
    println!("try: cargo run -- exec client -- ip addr");
    println!("delete with: cargo run -- down");
    Ok(())
}

fn run_down(args: &[String]) -> Result<()> {
    let topology = topology_from_args(args)?;
    println!("deleting topology");
    apply_plan(&topology.cleanup_plan())?;
    println!("DOWN: topology cleanup completed.");
    Ok(())
}

fn run_status(args: &[String]) -> Result<()> {
    if env::consts::OS != "linux" {
        return Err(virtualfacility::FacilityError::UnsupportedPlatform {
            current: env::consts::OS,
        });
    }
    let topology = topology_from_args(args)?;
    let netns_output = ProcessCommand::new("ip")
        .args(["netns", "list"])
        .output()
        .map_err(|err| virtualfacility::FacilityError::CommandFailed {
            command: "ip netns list".to_string(),
            code: None,
            stderr: err.to_string(),
        })?;
    if !netns_output.status.success() {
        return Err(virtualfacility::FacilityError::CommandFailed {
            command: "ip netns list".to_string(),
            code: netns_output.status.code(),
            stderr: String::from_utf8_lossy(&netns_output.stderr).into_owned(),
        });
    }
    let netns_list = String::from_utf8_lossy(&netns_output.stdout);
    let bridge_output = ProcessCommand::new("ip")
        .args(["link", "show", topology.bridge_name()])
        .output()
        .map_err(|err| virtualfacility::FacilityError::CommandFailed {
            command: format!("ip link show {}", topology.bridge_name()),
            code: None,
            stderr: err.to_string(),
        })?;

    let bridge_present = bridge_output.status.success();
    let node_statuses = topology
        .nodes()
        .iter()
        .map(|node| {
            let netns = format!("vf-{}-node-{}", topology.name(), node.name());
            let present = netns_list.contains(&netns);
            (node, netns, present)
        })
        .collect::<Vec<_>>();
    let pod_statuses = topology
        .pods()
        .iter()
        .map(|pod| {
            let netns = topology.pod_namespace(pod.name())?;
            let present = netns_list.contains(&netns);
            Ok((pod, netns, present))
        })
        .collect::<Result<Vec<_>>>()?;
    let present_count = usize::from(bridge_present)
        + node_statuses
            .iter()
            .filter(|(_, _, present)| *present)
            .count()
        + pod_statuses
            .iter()
            .filter(|(_, _, present)| *present)
            .count();
    let total_count = 1 + node_statuses.len() + pod_statuses.len();
    let state = match present_count {
        0 => "down",
        count if count == total_count => "running",
        _ => "partial",
    };

    println!("STATE: {state}");
    if state == "down" {
        println!("hint: run `cargo run -- up` to create this topology");
    }
    println!();

    println!("FACILITY  BRIDGE  BRIDGE-IP");
    println!(
        "{:<8}  {:<6}  {}",
        topology.name(),
        topology.bridge_name(),
        topology.bridge_cidr()
    );
    println!();

    println!("NODES");
    println!("{:<14}  {:<32}  {:<14}  STATUS", "NAME", "NETNS", "UPLINK");
    for (node, netns, present) in node_statuses {
        println!(
            "{:<14}  {:<32}  {:<14}  {}",
            node.name(),
            netns,
            topology.node_uplink_cidr(node),
            present_missing(present)
        );
    }
    println!();

    println!("PODS");
    println!(
        "{:<10}  {:<14}  {:<32}  {:<14}  STATUS",
        "NAME", "NODE", "NETNS", "IP"
    );
    for (pod, netns, present) in pod_statuses {
        println!(
            "{:<10}  {:<14}  {:<32}  {:<14}  {}",
            pod.name(),
            pod.node(),
            netns,
            topology.pod_cidr(pod),
            present_missing(present)
        );
    }
    println!();

    println!("NETWORK");
    println!("{:<10}  STATUS", "NAME");
    println!(
        "{:<10}  {}",
        topology.bridge_name(),
        present_missing(bridge_present)
    );
    Ok(())
}

fn present_missing(present: bool) -> &'static str {
    if present {
        "present"
    } else {
        "missing"
    }
}

fn run_ping(args: &[String]) -> Result<()> {
    let topology = topology_from_args(args)?;
    let op_args = operation_args(args);
    let source = op_args.first().map(String::as_str).unwrap_or("client");
    let target = op_args.get(1).map(String::as_str).unwrap_or("server");
    let source_ns = topology.pod_namespace(source)?;
    let target_ip =
        topology
            .resolve(target)
            .ok_or_else(|| virtualfacility::FacilityError::UnknownPod {
                name: target.to_string(),
            })?;
    println!("pinging {source} -> {target} ({target_ip})");
    let mut command = ProcessCommand::new("ip");
    command
        .args(["netns", "exec", source_ns.as_str(), "ping", "-c", "1"])
        .arg(target_ip.to_string());
    run_interactive(
        command,
        format!("ip netns exec {source_ns} ping -c 1 {target_ip}"),
    )
}

fn run_exec(args: &[String]) -> Result<()> {
    let topology = topology_from_args(args)?;
    let op_args = operation_args(args);
    if op_args.len() < 2 {
        eprintln!("{}", usage());
        process::exit(2);
    }
    let pod = &op_args[0];
    let command_args = if op_args.get(1).map(String::as_str) == Some("--") {
        &op_args[2..]
    } else {
        &op_args[1..]
    };
    if command_args.is_empty() {
        eprintln!("{}", usage());
        process::exit(2);
    }
    let pod_ns = topology.pod_namespace(pod)?;
    let mut command = ProcessCommand::new("ip");
    command
        .args(["netns", "exec", pod_ns.as_str()])
        .args(command_args);
    run_interactive(
        command,
        format!("ip netns exec {pod_ns} {}", command_args.join(" ")),
    )
}

fn run_create(args: &[String]) -> Result<()> {
    let op_args = operation_args(args);
    if op_args.is_empty() {
        eprintln!("{}", usage());
        process::exit(2);
    }
    let bridge_context = if op_args.first().map(String::as_str) == Some("bridge") {
        Some(bridge_context_or_exit(args, &op_args, "create"))
    } else {
        None
    };
    let effective_args = bridge_context
        .as_ref()
        .map(|context| args_with_context(&context.name, &context.bridge))
        .unwrap_or_else(|| args.to_vec());
    let topology = topology_from_args(&effective_args)?;
    match op_args[0].as_str() {
        "bridge" => {
            println!(
                "creating bridge {} for facility {}",
                topology.bridge_name(),
                topology.name()
            );
            apply_plan(&topology.bridge_setup_plan())?;
            if let Some(context) = bridge_context {
                write_context(&context)?;
                println!("current facility: {}", context.name);
                println!("current bridge: {}", context.bridge);
            }
        }
        "node" => {
            let node = op_args.get(1).map(String::as_str).unwrap_or("default-node");
            println!("creating node {node} for facility {}", topology.name());
            apply_plan(&topology.node_setup_plan(node)?)?;
        }
        "pod" => {
            let Some(pod) = op_args.get(1).map(String::as_str) else {
                eprintln!("{}", usage());
                process::exit(2);
            };
            println!("creating pod {pod} for facility {}", topology.name());
            apply_plan(&topology.pod_setup_plan(pod)?)?;
        }
        other => {
            eprintln!("unknown create target `{other}`");
            eprintln!("{}", usage());
            process::exit(2);
        }
    }
    Ok(())
}

fn run_delete(args: &[String]) -> Result<()> {
    let op_args = operation_args(args);
    if op_args.is_empty() {
        eprintln!("{}", usage());
        process::exit(2);
    }
    let bridge_context = if op_args.first().map(String::as_str) == Some("bridge") {
        Some(bridge_context_or_exit(args, &op_args, "delete"))
    } else {
        None
    };
    let effective_args = bridge_context
        .as_ref()
        .map(|context| args_with_context(&context.name, &context.bridge))
        .unwrap_or_else(|| args.to_vec());
    let topology = topology_from_args(&effective_args)?;
    match op_args[0].as_str() {
        "bridge" => {
            println!(
                "deleting bridge {} for facility {}",
                topology.bridge_name(),
                topology.name()
            );
            apply_plan(&topology.bridge_cleanup_plan())?;
        }
        "node" => {
            let node = op_args.get(1).map(String::as_str).unwrap_or("default-node");
            println!("deleting node {node} for facility {}", topology.name());
            apply_plan(&topology.node_cleanup_plan(node)?)?;
        }
        "pod" => {
            let Some(pod) = op_args.get(1).map(String::as_str) else {
                eprintln!("{}", usage());
                process::exit(2);
            };
            println!("deleting pod {pod} for facility {}", topology.name());
            apply_plan(&topology.pod_cleanup_plan(pod)?)?;
        }
        other => {
            eprintln!("unknown delete target `{other}`");
            eprintln!("{}", usage());
            process::exit(2);
        }
    }
    Ok(())
}

fn run_use(args: &[String]) -> Result<()> {
    let topology = topology_from_args(args)?;
    let op_args = operation_args(args);
    let name = op_args
        .first()
        .cloned()
        .unwrap_or_else(|| topology.name().to_string());
    let bridge = explicit_bridge(args).unwrap_or_else(|| default_bridge_for_name(&name));
    let context = FacilityContext { name, bridge };
    write_context(&context)?;
    println!("current facility: {}", context.name);
    println!("current bridge: {}", context.bridge);
    println!("saved in {CONTEXT_FILE}");
    Ok(())
}

fn run_current() -> Result<()> {
    let context = read_context();
    println!("current facility: {}", context.name);
    println!("current bridge: {}", context.bridge);
    println!("context file: {CONTEXT_FILE}");
    Ok(())
}

fn run_interactive(mut command: ProcessCommand, display: String) -> Result<()> {
    let status = command
        .status()
        .map_err(|err| virtualfacility::FacilityError::CommandFailed {
            command: display.clone(),
            code: None,
            stderr: err.to_string(),
        })?;
    if !status.success() {
        return Err(virtualfacility::FacilityError::CommandFailed {
            command: display,
            code: status.code(),
            stderr: "command exited unsuccessfully".to_string(),
        });
    }
    Ok(())
}

fn run_smoke(confirmed: bool, args: &[String]) -> Result<()> {
    let topology = topology_from_args(args)?;
    let setup = topology.setup_plan();
    let ping = topology.ping_plan("client", "server")?;
    let cleanup = topology.cleanup_plan();
    if !confirmed {
        let mut plan = CommandPlan::new();
        plan.extend(setup);
        plan.extend(ping);
        plan.extend(cleanup);
        print_plan(plan);
        return Ok(());
    }

    println!("creating smoke topology: 1 node, 3 pods");
    if let Err(err) = apply_plan(&setup) {
        eprintln!("setup failed; running cleanup");
        let _ = apply_plan(&cleanup);
        return Err(err);
    }
    println!("checking client pod can reach server pod");
    let ping_result = apply_plan(&ping);
    println!("cleaning smoke topology");
    let cleanup_result = apply_plan(&cleanup);
    ping_result?;
    cleanup_result?;
    println!("PASS: client pod reached server pod and cleanup completed.");
    Ok(())
}

fn usage() -> &'static str {
    "usage:
  virtualfacility [--name facility] [--bridge bridge] plan
  virtualfacility [--name facility] [--bridge bridge] up
  virtualfacility [--name facility] [--bridge bridge] create bridge [bridge-name]
  virtualfacility [--name facility] [--bridge bridge] create node [node-name]
  virtualfacility [--name facility] [--bridge bridge] create pod <pod-name>
  virtualfacility [--name facility] [--bridge bridge] status
  virtualfacility [--name facility] [--bridge bridge] exec <pod> -- <command> [args...]
  virtualfacility [--name facility] [--bridge bridge] ping [source-pod] [target-pod]
  virtualfacility [--name facility] [--bridge bridge] delete pod <pod-name>
  virtualfacility [--name facility] [--bridge bridge] delete node [node-name]
  virtualfacility [--name facility] [--bridge bridge] delete bridge [bridge-name]
  virtualfacility [--name facility] [--bridge bridge] down
  virtualfacility use <facility> [--bridge bridge]
  virtualfacility current
  virtualfacility workloads
  virtualfacility bootstrap [--i-understand]
  virtualfacility smoke [--i-understand]
  virtualfacility cleanup [--i-understand]
  virtualfacility check
  virtualfacility apply --i-understand

`up` creates the whole default topology and keeps it running. `create` and
`delete` let you manage bridge, node, and pod resources one at a time. `exec`
runs a command inside a pod namespace. `smoke --i-understand` is create, ping,
cleanup in one command. `bootstrap` still requires --i-understand because it
wraps the current test process in user, mount, and network namespaces.

Defaults: --name smoke --bridge vf-br0. `use` saves a local context so later
commands can omit --name and --bridge. `create bridge vf-lab1` infers
--name lab1 --bridge vf-lab1 and saves that context."
}
