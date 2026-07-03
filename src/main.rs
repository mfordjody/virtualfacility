use std::env;
use std::fs;
use std::net::Ipv4Addr;
use std::process::{self, Command as ProcessCommand};

use virtualfacility::{
    apply_plan, apply_rootless_bootstrap_plan, support_report, CommandPlan, CommandSpec, Result,
    RootlessBootstrap, Topology,
};

const CONTEXT_FILE: &str = ".virtualfacility-context";
const STATE_FILE: &str = ".virtualfacility-state";
const DEFAULT_NODE: &str = "default-node";
const DEFAULT_POD_SLOTS: [(&str, usize); 3] = [("proxy", 0), ("server", 1), ("client", 2)];

#[derive(Debug, Clone)]
struct FacilityContext {
    name: String,
    bridge: String,
}

#[derive(Debug, Clone)]
struct NodeState {
    name: String,
    slot: usize,
}

#[derive(Debug, Clone)]
struct PodState {
    name: String,
    node: String,
    slot: usize,
}

#[derive(Debug, Clone)]
struct FacilityState {
    context: FacilityContext,
    nodes: Vec<NodeState>,
    pods: Vec<PodState>,
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

fn empty_state(context: FacilityContext) -> FacilityState {
    FacilityState {
        context,
        nodes: Vec::new(),
        pods: Vec::new(),
    }
}

fn state_index(states: &[FacilityState], name: &str) -> Option<usize> {
    states.iter().position(|state| state.context.name == name)
}

fn upsert_state(states: &mut Vec<FacilityState>, state: FacilityState) {
    if let Some(index) = state_index(states, &state.context.name) {
        states[index] = state;
    } else {
        states.push(state);
    }
}

fn read_all_states() -> Vec<FacilityState> {
    let Ok(data) = fs::read_to_string(STATE_FILE) else {
        return Vec::new();
    };
    let mut states = Vec::new();
    let mut legacy_context = default_context();
    let mut legacy_nodes = Vec::new();
    let mut legacy_pods = Vec::new();
    let mut saw_legacy_context = false;

    for line in data.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "facility" => {
                let parts = value.split(',').collect::<Vec<_>>();
                let [name, bridge] = parts.as_slice() else {
                    continue;
                };
                upsert_state(
                    &mut states,
                    empty_state(FacilityContext {
                        name: (*name).to_string(),
                        bridge: (*bridge).to_string(),
                    }),
                );
            }
            "name" if !value.is_empty() => {
                legacy_context.name = value.to_string();
                saw_legacy_context = true;
            }
            "bridge" if !value.is_empty() => {
                legacy_context.bridge = value.to_string();
                saw_legacy_context = true;
            }
            "node" => {
                let parts = value.split(',').collect::<Vec<_>>();
                match parts.as_slice() {
                    [facility, name, slot] => {
                        let Ok(slot) = slot.parse::<usize>() else {
                            continue;
                        };
                        let index = if let Some(index) = state_index(&states, facility) {
                            index
                        } else {
                            states.push(empty_state(FacilityContext {
                                name: (*facility).to_string(),
                                bridge: default_bridge_for_name(facility),
                            }));
                            states.len() - 1
                        };
                        states[index].nodes.push(NodeState {
                            name: (*name).to_string(),
                            slot,
                        });
                    }
                    [name, slot] => {
                        let Ok(slot) = slot.parse::<usize>() else {
                            continue;
                        };
                        legacy_nodes.push(NodeState {
                            name: (*name).to_string(),
                            slot,
                        });
                    }
                    _ => {}
                }
            }
            "pod" => {
                let parts = value.split(',').collect::<Vec<_>>();
                match parts.as_slice() {
                    [facility, name, node, slot] => {
                        let Ok(slot) = slot.parse::<usize>() else {
                            continue;
                        };
                        let index = if let Some(index) = state_index(&states, facility) {
                            index
                        } else {
                            states.push(empty_state(FacilityContext {
                                name: (*facility).to_string(),
                                bridge: default_bridge_for_name(facility),
                            }));
                            states.len() - 1
                        };
                        states[index].pods.push(PodState {
                            name: (*name).to_string(),
                            node: (*node).to_string(),
                            slot,
                        });
                    }
                    [name, node, slot] => {
                        let Ok(slot) = slot.parse::<usize>() else {
                            continue;
                        };
                        legacy_pods.push(PodState {
                            name: (*name).to_string(),
                            node: (*node).to_string(),
                            slot,
                        });
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if saw_legacy_context || !legacy_nodes.is_empty() || !legacy_pods.is_empty() {
        upsert_state(
            &mut states,
            FacilityState {
                context: legacy_context,
                nodes: legacy_nodes,
                pods: legacy_pods,
            },
        );
    }
    states
}

fn read_state(context: &FacilityContext) -> FacilityState {
    read_all_states()
        .into_iter()
        .find(|state| state.context.name == context.name && state.context.bridge == context.bridge)
        .unwrap_or_else(|| empty_state(context.clone()))
}

fn write_all_states(states: &[FacilityState]) -> Result<()> {
    let mut data = String::new();
    for state in states {
        data.push_str(&format!(
            "facility={},{}\n",
            state.context.name, state.context.bridge
        ));
        for node in &state.nodes {
            data.push_str(&format!(
                "node={},{},{}\n",
                state.context.name, node.name, node.slot
            ));
        }
        for pod in &state.pods {
            data.push_str(&format!(
                "pod={},{},{},{}\n",
                state.context.name, pod.name, pod.node, pod.slot
            ));
        }
    }
    fs::write(STATE_FILE, data).map_err(|err| virtualfacility::FacilityError::CommandFailed {
        command: format!("write {STATE_FILE}"),
        code: None,
        stderr: err.to_string(),
    })
}

fn write_state(state: &FacilityState) -> Result<()> {
    let mut states = read_all_states();
    upsert_state(&mut states, state.clone());
    write_all_states(&states)
}

fn next_node_slot(state: &FacilityState) -> usize {
    state
        .nodes
        .iter()
        .map(|node| node.slot)
        .max()
        .map(|slot| slot + 1)
        .unwrap_or(0)
}

fn remember_node(state: &mut FacilityState, name: &str) {
    if state.nodes.iter().any(|node| node.name == name) {
        return;
    }
    state.nodes.push(NodeState {
        name: name.to_string(),
        slot: next_node_slot(state),
    });
}

fn forget_node(state: &mut FacilityState, name: &str) {
    state.nodes.retain(|node| node.name != name);
    state.pods.retain(|pod| pod.node != name);
}

fn is_default_pod(name: &str) -> bool {
    DEFAULT_POD_SLOTS
        .iter()
        .any(|(default_name, _)| *default_name == name)
}

fn next_pod_slot(state: &FacilityState) -> usize {
    state
        .pods
        .iter()
        .map(|pod| pod.slot)
        .chain(DEFAULT_POD_SLOTS.iter().map(|(_, slot)| *slot))
        .max()
        .map(|slot| slot + 1)
        .unwrap_or(0)
}

fn remember_pod(state: &mut FacilityState, name: &str, node: &str) {
    if is_default_pod(name) || state.pods.iter().any(|pod| pod.name == name) {
        return;
    }
    state.pods.push(PodState {
        name: name.to_string(),
        node: node.to_string(),
        slot: next_pod_slot(state),
    });
}

fn forget_pod(state: &mut FacilityState, name: &str) {
    state.pods.retain(|pod| pod.name != name);
}

fn explicit_node(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--node" {
            return args.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

fn default_node_for_state(state: &FacilityState) -> String {
    if state.nodes.len() == 1 {
        return state.nodes[0].name.clone();
    }
    DEFAULT_NODE.to_string()
}

fn first_command_index(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" | "--bridge" | "--node" => {
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

fn context_from_args(args: &[String]) -> FacilityContext {
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
            "--node" => {
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

    FacilityContext {
        name: facility_name,
        bridge: bridge_name,
    }
}

fn topology_for_state(context: &FacilityContext, state: &FacilityState) -> Result<Topology> {
    let server_url = "http://10.244.2.2:8080".to_string();
    let mut builder = Topology::builder(context.name.clone())
        .bridge_name(context.bridge.clone())
        .bridge_addr(bridge_addr_for_name(&context.bridge));
    if state.nodes.is_empty() {
        builder = builder
            .add_node(DEFAULT_NODE)
            .add_workload_pod("proxy", "default-node", ["proxy", "run"])
            .add_workload_pod(
                "server",
                "default-node",
                ["python3", "-m", "http.server", "8080"],
            )
            .add_workload_pod("client", "default-node", ["curl".to_string(), server_url]);
    } else {
        for node in &state.nodes {
            builder = builder.add_node_with_index(&node.name, node.slot);
        }
        for (name, slot) in DEFAULT_POD_SLOTS {
            if state.nodes.iter().any(|node| node.name == DEFAULT_NODE) {
                builder = match name {
                    "proxy" => builder.add_workload_pod_with_index(
                        "proxy",
                        DEFAULT_NODE,
                        slot,
                        ["proxy".to_string(), "run".to_string()],
                    ),
                    "server" => builder.add_workload_pod_with_index(
                        "server",
                        DEFAULT_NODE,
                        slot,
                        [
                            "python3".to_string(),
                            "-m".to_string(),
                            "http.server".to_string(),
                            "8080".to_string(),
                        ],
                    ),
                    "client" => builder.add_workload_pod_with_index(
                        "client",
                        DEFAULT_NODE,
                        slot,
                        ["curl".to_string(), server_url.clone()],
                    ),
                    _ => builder,
                };
            }
        }
    }
    for pod in &state.pods {
        builder = builder.add_pod_with_index(&pod.name, &pod.node, pod.slot);
    }
    builder.build()
}

fn bridge_addr_for_name(bridge: &str) -> Ipv4Addr {
    Ipv4Addr::new(10, 200, bridge_network_octet(bridge), 1)
}

fn bridge_network_octet(bridge: &str) -> u8 {
    if bridge == "vf-br0" {
        return 0;
    }
    if let Some(value) = trailing_number(bridge) {
        let slot = value % 240;
        return if slot == 0 { 1 } else { slot as u8 };
    }
    let hash = bridge.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u32)
    });
    (hash % 240 + 1) as u8
}

fn trailing_number(value: &str) -> Option<u32> {
    let digits = value
        .chars()
        .rev()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.chars().rev().collect::<String>().parse().ok()
}

fn topology_from_args(args: &[String]) -> Result<Topology> {
    let context = context_from_args(args);
    let state = read_state(&context);
    topology_for_state(&context, &state)
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

fn is_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "plan"
            | "up"
            | "create"
            | "delete"
            | "status"
            | "exec"
            | "ping"
            | "down"
            | "use"
            | "current"
            | "workloads"
            | "bootstrap"
            | "smoke"
            | "cleanup"
            | "check"
            | "apply"
            | "help"
    )
}

fn validate_new_bridge_name(bridge: &str) {
    if is_reserved_word(bridge) {
        fail_usage(format!(
            "invalid bridge name `{bridge}`: command words cannot be used as resource names"
        ));
    }
    if !bridge.starts_with("vf-") {
        fail_usage(format!(
            "invalid bridge name `{bridge}`: bridge names must start with `vf-`, for example `vf-lab1`"
        ));
    }
}

fn args_with_context(name: &str, bridge: &str) -> Vec<String> {
    vec![
        "--name".to_string(),
        name.to_string(),
        "--bridge".to_string(),
        bridge.to_string(),
    ]
}

fn fail_usage(message: impl AsRef<str>) -> ! {
    eprintln!("{}", message.as_ref());
    process::exit(2);
}

fn reject_extra_args(command: &str, extra: &[String]) -> ! {
    fail_usage(format!(
        "{command} accepts one target only; unexpected extra arguments: {}",
        extra.join(" ")
    ))
}

fn require_exact_args(op_args: &[String], command: &str, expected_len: usize) {
    if op_args.len() < expected_len {
        fail_usage(format!("{command} requires a target name"));
    }
    if op_args.len() > expected_len {
        reject_extra_args(command, &op_args[expected_len..]);
    }
}

fn bridge_context_or_exit(args: &[String], op_args: &[String], action: &str) -> FacilityContext {
    if op_args.len() > 2 {
        reject_extra_args(&format!("{action} bridge"), &op_args[2..]);
    }
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

fn ensure_linux() -> Result<()> {
    if env::consts::OS != "linux" {
        return Err(virtualfacility::FacilityError::UnsupportedPlatform {
            current: env::consts::OS,
        });
    }
    Ok(())
}

fn netns_names() -> Result<Vec<String>> {
    ensure_linux()?;
    let output = ProcessCommand::new("ip")
        .args(["netns", "list"])
        .output()
        .map_err(|err| virtualfacility::FacilityError::CommandFailed {
            command: "ip netns list".to_string(),
            code: None,
            stderr: err.to_string(),
        })?;
    if !output.status.success() {
        return Err(virtualfacility::FacilityError::CommandFailed {
            command: "ip netns list".to_string(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next().map(str::to_string))
        .collect())
}

fn bridge_exists(bridge: &str) -> Result<bool> {
    ensure_linux()?;
    let output = ProcessCommand::new("ip")
        .args(["link", "show", bridge])
        .output()
        .map_err(|err| virtualfacility::FacilityError::CommandFailed {
            command: format!("ip link show {bridge}"),
            code: None,
            stderr: err.to_string(),
        })?;
    Ok(output.status.success())
}

fn bridge_names() -> Result<Vec<String>> {
    ensure_linux()?;
    let output = ProcessCommand::new("ip")
        .args(["-o", "link", "show", "type", "bridge"])
        .output()
        .map_err(|err| virtualfacility::FacilityError::CommandFailed {
            command: "ip -o link show type bridge".to_string(),
            code: None,
            stderr: err.to_string(),
        })?;
    if !output.status.success() {
        return Err(virtualfacility::FacilityError::CommandFailed {
            command: "ip -o link show type bridge".to_string(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(|name| name.trim_end_matches(':').to_string())
        .collect())
}

fn bridge_ipv4_cidr(bridge: &str) -> Result<Option<String>> {
    ensure_linux()?;
    let output = ProcessCommand::new("ip")
        .args(["-o", "-4", "addr", "show", "dev", bridge, "scope", "global"])
        .output()
        .map_err(|err| virtualfacility::FacilityError::CommandFailed {
            command: format!("ip -o -4 addr show dev {bridge} scope global"),
            code: None,
            stderr: err.to_string(),
        })?;
    if !output.status.success() {
        return Err(virtualfacility::FacilityError::CommandFailed {
            command: format!("ip -o -4 addr show dev {bridge} scope global"),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            while let Some(part) = parts.next() {
                if part == "inet" {
                    return parts.next().map(str::to_string);
                }
            }
            None
        })
        .next())
}

fn node_namespace(topology: &Topology, node_name: &str) -> Result<String> {
    topology
        .nodes()
        .iter()
        .find(|node| node.name() == node_name)
        .map(|node| format!("vf-{}-node-{}", topology.name(), node.name()))
        .ok_or_else(|| virtualfacility::FacilityError::UnknownNodeName {
            name: node_name.to_string(),
        })
}

fn require_bridge_exists(bridge: &str) -> Result<()> {
    if bridge_exists(bridge)? {
        return Ok(());
    }
    eprintln!("bridge `{bridge}` does not exist");
    process::exit(1);
}

fn require_netns_exists(kind: &str, name: &str, netns: &str, netns_list: &[String]) {
    if netns_list.iter().any(|existing| existing == netns) {
        return;
    }
    eprintln!("{kind} `{name}` does not exist: namespace `{netns}` was not found");
    process::exit(1);
}

fn facility_netns_prefixes(facility: &str) -> (String, String) {
    (
        format!("vf-{facility}-node-"),
        format!("vf-{facility}-pod-"),
    )
}

fn operation_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" | "--bridge" | "--node" => {
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

fn has_explicit_scope(args: &[String]) -> bool {
    explicit_name(args).is_some() || explicit_bridge(args).is_some()
}

fn ensure_status_state(states: &mut Vec<FacilityState>, context: FacilityContext) {
    if state_index(states, &context.name).is_none() {
        states.push(empty_state(context));
    }
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
    ensure_linux()?;
    let netns_list = netns_names()?;
    let bridge_list = bridge_names()?;
    let bridge_set = bridge_list.iter().collect::<Vec<_>>();
    let mut states = if has_explicit_scope(args) {
        let context = context_from_args(args);
        vec![read_state(&context)]
    } else {
        let mut states = read_all_states();
        ensure_status_state(&mut states, read_context());
        for bridge in &bridge_list {
            if bridge.starts_with("vf-") {
                ensure_status_state(
                    &mut states,
                    FacilityContext {
                        name: facility_from_bridge(bridge),
                        bridge: bridge.clone(),
                    },
                );
            }
        }
        states
    };

    for state in &mut states {
        let (node_prefix, pod_prefix) = facility_netns_prefixes(&state.context.name);
        for netns in &netns_list {
            if let Some(node_name) = netns.strip_prefix(&node_prefix) {
                remember_node(state, node_name);
                continue;
            }
            let Some(pod_name) = netns.strip_prefix(&pod_prefix) else {
                continue;
            };
            let node = default_node_for_state(state);
            remember_pod(state, pod_name, &node);
        }
    }

    if !has_explicit_scope(args) {
        write_all_states(&states)?;
    }

    let mut network_rows = Vec::new();
    let mut node_rows = Vec::new();
    let mut pod_rows = Vec::new();
    for state in &states {
        let topology = topology_for_state(&state.context, state)?;
        let bridge_present = bridge_set
            .iter()
            .any(|bridge| bridge.as_str() == topology.bridge_name());
        if bridge_present {
            let cidr =
                bridge_ipv4_cidr(topology.bridge_name())?.unwrap_or_else(|| topology.bridge_cidr());
            network_rows.push((topology.bridge_name().to_string(), cidr));
        }
        for node in topology.nodes() {
            let netns = format!("vf-{}-node-{}", topology.name(), node.name());
            if netns_list.iter().any(|existing| existing == &netns) {
                node_rows.push((
                    topology.bridge_name().to_string(),
                    node.name().to_string(),
                    netns,
                    topology.node_uplink_cidr(node),
                ));
            }
        }
        for pod in topology.pods() {
            let netns = topology.pod_namespace(pod.name())?;
            if netns_list.iter().any(|existing| existing == &netns) {
                pod_rows.push((
                    topology.bridge_name().to_string(),
                    pod.name().to_string(),
                    pod.node().to_string(),
                    netns,
                    topology.pod_cidr(pod),
                ));
            }
        }
    }

    let actual_count = network_rows.len() + node_rows.len() + pod_rows.len();
    let state = if actual_count == 0 {
        "down"
    } else if !network_rows.is_empty() && !node_rows.is_empty() && !pod_rows.is_empty() {
        "running"
    } else {
        "partial"
    };

    println!("STATE: {state}");
    if state == "down" {
        println!("hint: run `cargo run -- up` to create this topology");
    }
    println!();

    println!("NETWORKS");
    if network_rows.is_empty() {
        println!("No networks found.");
    } else {
        println!("{:<14}  {:<14}  STATUS", "NAME", "BRIDGE-IP");
        for (bridge, cidr) in network_rows {
            println!("{bridge:<14}  {cidr:<14}  present");
        }
    }
    println!();

    println!("NODES");
    if node_rows.is_empty() {
        println!("No nodes found.");
    } else {
        println!(
            "{:<10}  {:<14}  {:<32}  {:<14}  STATUS",
            "NETWORK", "NAME", "NETNS", "UPLINK"
        );
        for (network, node, netns, uplink) in node_rows {
            println!(
                "{:<10}  {:<14}  {:<32}  {:<14}  present",
                network, node, netns, uplink,
            );
        }
    }
    println!();

    println!("PODS");
    if pod_rows.is_empty() {
        println!("No pods found.");
    } else {
        println!(
            "{:<10}  {:<10}  {:<14}  {:<32}  {:<14}  STATUS",
            "NETWORK", "NAME", "NODE", "NETNS", "IP"
        );
        for (network, pod, node, netns, ip) in pod_rows {
            println!(
                "{:<10}  {:<10}  {:<14}  {:<32}  {:<14}  present",
                network, pod, node, netns, ip,
            );
        }
    }
    Ok(())
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
    let context = context_from_args(&effective_args);
    let mut state = read_state(&context);
    match op_args[0].as_str() {
        "bridge" => {
            if let Some(context) = &bridge_context {
                validate_new_bridge_name(&context.bridge);
            }
            let topology = topology_for_state(&context, &state)?;
            if bridge_exists(topology.bridge_name())? {
                println!(
                    "bridge {} for environment {} already exists",
                    topology.bridge_name(),
                    topology.name()
                );
                if let Some(context) = bridge_context {
                    write_context(&context)?;
                    state.context = context.clone();
                    write_state(&state)?;
                    println!("current environment: {}", context.name);
                    println!("current bridge: {}", context.bridge);
                }
                return Ok(());
            }
            println!(
                "creating bridge {} for environment {}",
                topology.bridge_name(),
                topology.name()
            );
            apply_plan(&topology.bridge_setup_plan())?;
            if let Some(context) = bridge_context {
                write_context(&context)?;
                state.context = context.clone();
                write_state(&state)?;
                println!("current environment: {}", context.name);
                println!("current bridge: {}", context.bridge);
            }
        }
        "node" => {
            if op_args.len() > 2 {
                reject_extra_args("create node", &op_args[2..]);
            }
            let node = op_args.get(1).map(String::as_str).unwrap_or("default-node");
            remember_node(&mut state, node);
            let topology = topology_for_state(&context, &state)?;
            require_bridge_exists(topology.bridge_name())?;
            let netns = node_namespace(&topology, node)?;
            let existing_netns = netns_names()?;
            if existing_netns.iter().any(|existing| existing == &netns) {
                println!(
                    "node {node} for environment {} already exists",
                    topology.name()
                );
                write_state(&state)?;
                return Ok(());
            }
            let plan = topology.node_setup_plan(node)?;
            println!("creating node {node} for environment {}", topology.name());
            apply_plan(&plan)?;
            write_state(&state)?;
        }
        "pod" => {
            require_exact_args(&op_args, "create pod", 2);
            let pod = &op_args[1];
            let node = explicit_node(args).unwrap_or_else(|| default_node_for_state(&state));
            remember_node(&mut state, &node);
            remember_pod(&mut state, pod, &node);
            let topology = topology_for_state(&context, &state)?;
            require_bridge_exists(topology.bridge_name())?;
            let node_netns = node_namespace(&topology, &node)?;
            let existing_netns = netns_names()?;
            require_netns_exists("node", &node, &node_netns, &existing_netns);
            let pod_netns = topology.pod_namespace(pod)?;
            if existing_netns.iter().any(|existing| existing == &pod_netns) {
                println!(
                    "pod {pod} for environment {} already exists",
                    topology.name()
                );
                write_state(&state)?;
                return Ok(());
            }
            let plan = topology.pod_setup_plan(pod)?;
            println!("creating pod {pod} for environment {}", topology.name());
            apply_plan(&plan)?;
            write_state(&state)?;
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
    let context = context_from_args(&effective_args);
    let mut state = read_state(&context);
    match op_args[0].as_str() {
        "bridge" => {
            let topology = topology_for_state(&context, &state)?;
            require_bridge_exists(topology.bridge_name())?;
            let existing_netns = netns_names()?;
            let (node_prefix, pod_prefix) = facility_netns_prefixes(topology.name());
            let dependent_netns = existing_netns
                .into_iter()
                .filter(|netns| netns.starts_with(&node_prefix) || netns.starts_with(&pod_prefix))
                .collect::<Vec<_>>();
            if !dependent_netns.is_empty() {
                eprintln!(
                    "cannot delete bridge `{}` while namespaces still exist: {}",
                    topology.bridge_name(),
                    dependent_netns.join(", ")
                );
                eprintln!("delete pods and nodes first, or run `cargo run -- down`");
                process::exit(1);
            }
            println!(
                "deleting bridge {} for environment {}",
                topology.bridge_name(),
                topology.name()
            );
            apply_plan(&topology.bridge_cleanup_plan())?;
            state.pods.clear();
            write_state(&state)?;
        }
        "node" => {
            require_exact_args(&op_args, "delete node", 2);
            let node = &op_args[1];
            remember_node(&mut state, node);
            let topology = topology_for_state(&context, &state)?;
            let plan = topology.node_cleanup_plan(node)?;
            let netns = node_namespace(&topology, node)?;
            let existing_netns = netns_names()?;
            require_netns_exists("node", node, &netns, &existing_netns);
            println!("deleting node {node} for environment {}", topology.name());
            apply_plan(&plan)?;
            forget_node(&mut state, node);
            write_state(&state)?;
        }
        "pod" => {
            require_exact_args(&op_args, "delete pod", 2);
            let pod = &op_args[1];
            let node = explicit_node(args).unwrap_or_else(|| default_node_for_state(&state));
            remember_node(&mut state, &node);
            remember_pod(&mut state, pod, &node);
            let topology = topology_for_state(&context, &state)?;
            let plan = topology.pod_cleanup_plan(pod)?;
            let netns = topology.pod_namespace(pod)?;
            let existing_netns = netns_names()?;
            require_netns_exists("pod", pod, &netns, &existing_netns);
            println!("deleting pod {pod} for environment {}", topology.name());
            apply_plan(&plan)?;
            forget_pod(&mut state, pod);
            write_state(&state)?;
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
    println!("current environment: {}", context.name);
    println!("current bridge: {}", context.bridge);
    println!("saved in {CONTEXT_FILE}");
    Ok(())
}

fn run_current() -> Result<()> {
    let context = read_context();
    println!("current environment: {}", context.name);
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
  virtualfacility [--name env] [--bridge bridge] plan
  virtualfacility [--name env] [--bridge bridge] up
  virtualfacility [--name env] [--bridge bridge] create bridge <bridge-name>
  virtualfacility [--name env] [--bridge bridge] create node [node-name]
  virtualfacility [--name env] [--bridge bridge] create pod <pod-name> [--node node-name]
  virtualfacility [--name env] [--bridge bridge] status
  virtualfacility [--name env] [--bridge bridge] exec <pod> -- <command> [args...]
  virtualfacility [--name env] [--bridge bridge] ping [source-pod] [target-pod]
  virtualfacility [--name env] [--bridge bridge] delete pod <pod-name> [--node node-name]
  virtualfacility [--name env] [--bridge bridge] delete node <node-name>
  virtualfacility [--name env] [--bridge bridge] delete bridge <bridge-name>
  virtualfacility [--name env] [--bridge bridge] down
  virtualfacility use <env> [--bridge bridge]
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
--name lab1 --bridge vf-lab1 and saves that context. With multiple environments,
commands target the current context unless --name or --bridge is provided."
}
