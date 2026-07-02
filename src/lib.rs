use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::thread;

pub type Result<T> = std::result::Result<T, FacilityError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacilityError {
    CommandFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    DuplicateName {
        kind: &'static str,
        name: String,
    },
    EmptyWorkload {
        pod: String,
    },
    EmptyCommand {
        context: &'static str,
    },
    InvalidName {
        kind: &'static str,
        name: String,
        reason: &'static str,
    },
    NamespaceSyscall {
        syscall: &'static str,
        detail: String,
    },
    MissingCommand {
        command: &'static str,
    },
    ThreadPanicked,
    TooManyItems {
        kind: &'static str,
        max: usize,
        actual: usize,
    },
    UnknownNode {
        pod: String,
        node: String,
    },
    UnknownNodeName {
        name: String,
    },
    UnknownPod {
        name: String,
    },
    UnsupportedPlatform {
        current: &'static str,
    },
}

impl fmt::Display for FacilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandFailed {
                command,
                code,
                stderr,
            } => write!(
                f,
                "command failed: `{command}` exited with {:?}: {}",
                code,
                stderr.trim()
            ),
            Self::DuplicateName { kind, name } => {
                write!(f, "duplicate {kind} name `{name}`")
            }
            Self::EmptyWorkload { pod } => {
                write!(f, "pod `{pod}` has an empty workload command")
            }
            Self::EmptyCommand { context } => {
                write!(f, "{context} command must not be empty")
            }
            Self::InvalidName { kind, name, reason } => {
                write!(f, "invalid {kind} name `{name}`: {reason}")
            }
            Self::NamespaceSyscall { syscall, detail } => {
                write!(f, "{syscall} failed: {detail}")
            }
            Self::MissingCommand { command } => {
                write!(f, "required command `{command}` was not found in PATH")
            }
            Self::ThreadPanicked => {
                write!(f, "namespace thread panicked")
            }
            Self::TooManyItems { kind, max, actual } => {
                write!(f, "too many {kind}: max {max}, got {actual}")
            }
            Self::UnknownNode { pod, node } => {
                write!(f, "pod `{pod}` references unknown node `{node}`")
            }
            Self::UnknownNodeName { name } => {
                write!(f, "unknown node `{name}`")
            }
            Self::UnknownPod { name } => {
                write!(f, "unknown pod `{name}`")
            }
            Self::UnsupportedPlatform { current } => {
                write!(f, "Linux namespace execution is unsupported on `{current}`")
            }
        }
    }
}

impl Error for FacilityError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    program: String,
    args: Vec<String>,
}

impl CommandSpec {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn as_shell(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Bootstrap,
    Setup,
    Workload,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    pub stage: Stage,
    pub description: String,
    pub command: CommandSpec,
    pub allow_failure: bool,
}

impl PlanStep {
    pub fn strict(stage: Stage, description: impl Into<String>, command: CommandSpec) -> Self {
        Self {
            stage,
            description: description.into(),
            command,
            allow_failure: false,
        }
    }

    pub fn best_effort(stage: Stage, description: impl Into<String>, command: CommandSpec) -> Self {
        Self {
            stage,
            description: description.into(),
            command,
            allow_failure: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandPlan {
    steps: Vec<PlanStep>,
}

impl CommandPlan {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    pub fn push(&mut self, step: PlanStep) {
        self.steps.push(step);
    }

    pub fn extend(&mut self, other: CommandPlan) {
        self.steps.extend(other.steps);
    }

    pub fn steps(&self) -> &[PlanStep] {
        &self.steps
    }

    pub fn render_shell(&self) -> String {
        let mut rendered = String::from("set -eu\n");
        for step in &self.steps {
            rendered.push_str("\n# ");
            rendered.push_str(&step.description);
            rendered.push('\n');
            rendered.push_str(&step.command.as_shell());
            if step.allow_failure {
                rendered.push_str(" || true");
            }
            rendered.push('\n');
        }
        rendered
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindMount {
    source: PathBuf,
    target: PathBuf,
    read_only: bool,
    kind: BindMountKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindMountKind {
    File,
    Directory,
}

impl BindMount {
    pub fn file(source: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            read_only: false,
            kind: BindMountKind::File,
        }
    }

    pub fn directory(source: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            read_only: false,
            kind: BindMountKind::Directory,
        }
    }

    pub fn read_only_file(source: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            read_only: true,
            kind: BindMountKind::File,
        }
    }

    pub fn read_only_directory(source: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            read_only: true,
            kind: BindMountKind::Directory,
        }
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    pub fn kind(&self) -> BindMountKind {
        self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootlessBootstrap {
    scratch_dir: PathBuf,
    bind_mounts: Vec<BindMount>,
    precreate_files: Vec<PathBuf>,
    precreate_dirs: Vec<PathBuf>,
}

impl RootlessBootstrap {
    pub fn new(scratch_dir: impl Into<PathBuf>) -> Self {
        Self {
            scratch_dir: scratch_dir.into(),
            bind_mounts: Vec::new(),
            precreate_files: Vec::new(),
            precreate_dirs: Vec::new(),
        }
    }

    pub fn with_bind_mount(mut self, bind: BindMount) -> Self {
        self.bind_mounts.push(bind);
        self
    }

    pub fn with_precreated_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.precreate_files.push(path.into());
        self
    }

    pub fn with_precreated_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.precreate_dirs.push(path.into());
        self
    }

    pub fn with_xtables_lock(mut self) -> Self {
        let source = self.scratch_dir.join("xtables.lock");
        self.bind_mounts
            .push(BindMount::file(source, "/run/xtables.lock"));
        self
    }

    pub fn with_ip_netns_dir(mut self) -> Self {
        let source = self.scratch_dir.join("netns");
        self.bind_mounts
            .push(BindMount::directory(source, "/var/run/netns"));
        self
    }

    pub fn with_standard_mounts(self) -> Self {
        let run_dir = self.scratch_dir.join("run");
        self.with_precreated_file(run_dir.join("xtables.lock"))
            .with_precreated_dir(run_dir.join("netns"))
            .with_bind_mount(BindMount::directory(run_dir, "/run"))
    }

    pub fn bootstrap_plan(&self, inner: CommandSpec) -> Result<CommandPlan> {
        if inner.program().is_empty() {
            return Err(FacilityError::EmptyCommand {
                context: "bootstrap inner",
            });
        }
        let mut plan = CommandPlan::new();
        plan.push(PlanStep::strict(
            Stage::Bootstrap,
            format!(
                "create bootstrap scratch directory `{}`",
                self.scratch_dir.display()
            ),
            CommandSpec::new("mkdir", ["-p".to_string(), self.scratch_dir_string()]),
        ));
        for dir in &self.precreate_dirs {
            plan.push(PlanStep::strict(
                Stage::Bootstrap,
                format!("create bootstrap directory `{}`", dir.display()),
                CommandSpec::new("mkdir", ["-p".to_string(), path_string(dir)]),
            ));
        }
        for file in &self.precreate_files {
            plan.push(PlanStep::strict(
                Stage::Bootstrap,
                format!("create bootstrap file `{}`", file.display()),
                CommandSpec::new("touch", [path_string(file)]),
            ));
        }
        for bind in &self.bind_mounts {
            let command = match bind.kind {
                BindMountKind::File => CommandSpec::new("touch", [path_string(&bind.source)]),
                BindMountKind::Directory => {
                    CommandSpec::new("mkdir", ["-p".to_string(), path_string(&bind.source)])
                }
            };
            plan.push(PlanStep::strict(
                Stage::Bootstrap,
                format!("create bind-mount source `{}`", bind.source.display()),
                command,
            ));
        }
        plan.push(PlanStep::strict(
            Stage::Bootstrap,
            "enter rootless user, mount, and network namespaces",
            self.unshare_command(inner),
        ));
        Ok(plan)
    }

    fn unshare_command(&self, inner: CommandSpec) -> CommandSpec {
        let mut args = vec![
            "--user".to_string(),
            "--map-root-user".to_string(),
            "--mount".to_string(),
            "--net".to_string(),
            "--fork".to_string(),
            "--".to_string(),
            "sh".to_string(),
            "-eu".to_string(),
            "-c".to_string(),
            self.bootstrap_script(),
            "virtualfacility-bootstrap".to_string(),
        ];
        for bind in &self.bind_mounts {
            args.push(path_string(&bind.source));
            args.push(path_string(&bind.target));
            args.push(if bind.read_only { "ro" } else { "rw" }.to_string());
            args.push(
                match bind.kind {
                    BindMountKind::File => "file",
                    BindMountKind::Directory => "dir",
                }
                .to_string(),
            );
        }
        args.push("--".to_string());
        args.push(inner.program().to_string());
        args.extend(inner.args().iter().cloned());
        CommandSpec::new("unshare", args)
    }

    fn bootstrap_script(&self) -> String {
        let bind_count = self.bind_mounts.len();
        format!(
            "mount --make-rprivate /; \
             i=0; \
             while [ \"$i\" -lt {bind_count} ]; do \
               src=\"$1\"; target=\"$2\"; mode=\"$3\"; kind=\"$4\"; shift 4; \
               if [ \"$kind\" = dir ]; then mkdir -p \"$src\"; else mkdir -p \"$(dirname \"$src\")\"; touch \"$src\"; fi; \
               mount --bind \"$src\" \"$target\"; \
               if [ \"$mode\" = ro ]; then mount -o remount,bind,ro \"$target\"; fi; \
               i=$((i + 1)); \
             done; \
             shift; \
             exec \"$@\""
        )
    }

    fn scratch_dir_string(&self) -> String {
        path_string(&self.scratch_dir)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topology {
    name: String,
    bridge_name: String,
    bridge_addr: Ipv4Addr,
    nodes: Vec<Node>,
    pods: Vec<Pod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    name: String,
    index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pod {
    name: String,
    node: String,
    index: usize,
    workload: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodAddress {
    pub pod_ip: Ipv4Addr,
    pub pod_cidr: String,
    pub node_side_ip: Ipv4Addr,
    pub node_side_cidr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyBuilder {
    name: String,
    bridge_name: String,
    bridge_addr: Ipv4Addr,
    nodes: Vec<PendingNode>,
    pods: Vec<PendingPod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingNode {
    name: String,
    index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingPod {
    name: String,
    node: String,
    index: Option<usize>,
    workload: Option<Vec<String>>,
}

impl Topology {
    pub fn builder(name: impl Into<String>) -> TopologyBuilder {
        TopologyBuilder {
            name: name.into(),
            bridge_name: "vf-br0".to_string(),
            bridge_addr: Ipv4Addr::new(10, 200, 0, 1),
            nodes: Vec::new(),
            pods: Vec::new(),
        }
    }

    pub fn smoke() -> Result<Self> {
        let server_url = "http://10.244.2.2:8080".to_string();
        Self::builder("smoke")
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

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn bridge_name(&self) -> &str {
        &self.bridge_name
    }

    pub fn bridge_cidr(&self) -> String {
        format!("{}/24", self.bridge_addr)
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn pods(&self) -> &[Pod] {
        &self.pods
    }

    pub fn resolve(&self, pod_name: &str) -> Option<Ipv4Addr> {
        self.pods
            .iter()
            .find(|pod| pod.name == pod_name)
            .map(|pod| self.pod_address(pod).pod_ip)
    }

    pub fn pod_namespace(&self, pod_name: &str) -> Result<String> {
        self.pod_by_name(pod_name)
            .map(|pod| pod.netns(&self.name))
            .ok_or_else(|| FacilityError::UnknownPod {
                name: pod_name.to_string(),
            })
    }

    pub fn node_namespace_names(&self) -> Vec<String> {
        self.nodes
            .iter()
            .map(|node| node.netns(&self.name))
            .collect()
    }

    pub fn pod_namespace_names(&self) -> Vec<String> {
        self.pods.iter().map(|pod| pod.netns(&self.name)).collect()
    }

    pub fn node_uplink_cidr(&self, node: &Node) -> String {
        format!("{}/24", self.node_uplink_ip(node))
    }

    pub fn pod_cidr(&self, pod: &Pod) -> String {
        self.pod_address(pod).pod_cidr
    }

    pub fn setup_plan(&self) -> CommandPlan {
        let mut plan = CommandPlan::new();
        self.bridge_setup(&mut plan);
        for node in &self.nodes {
            self.node_setup(&mut plan, node);
        }
        for pod in &self.pods {
            self.pod_setup(&mut plan, pod);
        }
        self.cross_node_routes(&mut plan);
        plan
    }

    pub fn bridge_setup_plan(&self) -> CommandPlan {
        let mut plan = CommandPlan::new();
        self.bridge_setup(&mut plan);
        plan
    }

    pub fn node_setup_plan(&self, node_name: &str) -> Result<CommandPlan> {
        let node = self
            .node_by_name(node_name)
            .ok_or_else(|| FacilityError::UnknownNodeName {
                name: node_name.to_string(),
            })?;
        let mut plan = CommandPlan::new();
        self.node_setup(&mut plan, node);
        Ok(plan)
    }

    pub fn pod_setup_plan(&self, pod_name: &str) -> Result<CommandPlan> {
        let pod = self
            .pod_by_name(pod_name)
            .ok_or_else(|| FacilityError::UnknownPod {
                name: pod_name.to_string(),
            })?;
        let mut plan = CommandPlan::new();
        self.pod_setup(&mut plan, pod);
        Ok(plan)
    }

    pub fn workload_plan(&self) -> Result<CommandPlan> {
        let mut plan = CommandPlan::new();
        for pod in &self.pods {
            if let Some(command) = &pod.workload {
                if command.is_empty() {
                    return Err(FacilityError::EmptyWorkload {
                        pod: pod.name.clone(),
                    });
                }
                let mut args = vec![
                    "netns".to_string(),
                    "exec".to_string(),
                    pod.netns(&self.name),
                ];
                args.extend(command.clone());
                plan.push(PlanStep::strict(
                    Stage::Workload,
                    format!("run workload `{}` inside pod `{}`", command[0], pod.name),
                    CommandSpec::new("ip", args),
                ));
            }
        }
        Ok(plan)
    }

    pub fn ping_plan(&self, source_pod: &str, target_pod: &str) -> Result<CommandPlan> {
        let source = self
            .pod_by_name(source_pod)
            .ok_or_else(|| FacilityError::UnknownPod {
                name: source_pod.to_string(),
            })?;
        let target_ip = self
            .resolve(target_pod)
            .ok_or_else(|| FacilityError::UnknownPod {
                name: target_pod.to_string(),
            })?;
        let mut plan = CommandPlan::new();
        plan.push(PlanStep::strict(
            Stage::Workload,
            format!("ping pod `{target_pod}` from pod `{source_pod}`"),
            CommandSpec::new(
                "ip",
                [
                    "netns".to_string(),
                    "exec".to_string(),
                    source.netns(&self.name),
                    "ping".to_string(),
                    "-c".to_string(),
                    "1".to_string(),
                    target_ip.to_string(),
                ],
            ),
        ));
        Ok(plan)
    }

    pub fn cleanup_plan(&self) -> CommandPlan {
        let mut plan = CommandPlan::new();
        for pod in self.pods.iter().rev() {
            plan.push(PlanStep::best_effort(
                Stage::Cleanup,
                format!("delete pod namespace `{}`", pod.netns(&self.name)),
                CommandSpec::new("ip", ["netns", "del", pod.netns(&self.name).as_str()]),
            ));
        }
        for node in self.nodes.iter().rev() {
            self.node_cleanup(&mut plan, node);
        }
        plan.push(PlanStep::best_effort(
            Stage::Cleanup,
            format!("delete bridge `{}`", self.bridge_name),
            CommandSpec::new("ip", ["link", "del", self.bridge_name.as_str()]),
        ));
        plan
    }

    pub fn bridge_cleanup_plan(&self) -> CommandPlan {
        let mut plan = CommandPlan::new();
        self.bridge_cleanup(&mut plan);
        plan
    }

    pub fn node_cleanup_plan(&self, node_name: &str) -> Result<CommandPlan> {
        let node = self
            .node_by_name(node_name)
            .ok_or_else(|| FacilityError::UnknownNodeName {
                name: node_name.to_string(),
            })?;
        let mut plan = CommandPlan::new();
        self.node_cleanup(&mut plan, node);
        Ok(plan)
    }

    pub fn pod_cleanup_plan(&self, pod_name: &str) -> Result<CommandPlan> {
        let pod = self
            .pod_by_name(pod_name)
            .ok_or_else(|| FacilityError::UnknownPod {
                name: pod_name.to_string(),
            })?;
        let mut plan = CommandPlan::new();
        self.pod_cleanup(&mut plan, pod);
        Ok(plan)
    }

    pub fn render_summary(&self) -> String {
        let mut lines = vec![format!("facility: {}", self.name)];
        lines.push(format!(
            "bridge: {} at {}/24",
            self.bridge_name, self.bridge_addr
        ));
        for node in &self.nodes {
            lines.push(format!(
                "node: {} netns={} uplink={}/24",
                node.name,
                node.netns(&self.name),
                self.node_uplink_ip(node)
            ));
        }
        for pod in &self.pods {
            let address = self.pod_address(pod);
            lines.push(format!(
                "pod: {} node={} netns={} ip={}",
                pod.name,
                pod.node,
                pod.netns(&self.name),
                address.pod_cidr
            ));
        }
        lines.join("\n")
    }

    fn bridge_setup(&self, plan: &mut CommandPlan) {
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!(
                "create bridge `{}` to stand in for the cluster underlay",
                self.bridge_name
            ),
            CommandSpec::new(
                "ip",
                ["link", "add", self.bridge_name.as_str(), "type", "bridge"],
            ),
        ));
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!("assign bridge address `{}/24`", self.bridge_addr),
            CommandSpec::new(
                "ip",
                [
                    "addr",
                    "add",
                    &format!("{}/24", self.bridge_addr),
                    "dev",
                    self.bridge_name.as_str(),
                ],
            ),
        ));
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!("bring bridge `{}` up", self.bridge_name),
            CommandSpec::new("ip", ["link", "set", self.bridge_name.as_str(), "up"]),
        ));
    }

    fn bridge_cleanup(&self, plan: &mut CommandPlan) {
        plan.push(PlanStep::best_effort(
            Stage::Cleanup,
            format!("delete bridge `{}`", self.bridge_name),
            CommandSpec::new("ip", ["link", "del", self.bridge_name.as_str()]),
        ));
    }

    fn node_setup(&self, plan: &mut CommandPlan, node: &Node) {
        let node_ns = node.netns(&self.name);
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!("create node namespace `{node_ns}`"),
            CommandSpec::new("ip", ["netns", "add", node_ns.as_str()]),
        ));
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!(
                "create veth pair `{}` <-> `{}` for node `{}`",
                node.host_veth(&self.name),
                node.node_veth(&self.name),
                node.name
            ),
            CommandSpec::new(
                "ip",
                [
                    "link",
                    "add",
                    node.host_veth(&self.name).as_str(),
                    "type",
                    "veth",
                    "peer",
                    "name",
                    node.node_veth(&self.name).as_str(),
                ],
            ),
        ));
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!("move `{}` into node namespace", node.node_veth(&self.name)),
            CommandSpec::new(
                "ip",
                [
                    "link",
                    "set",
                    node.node_veth(&self.name).as_str(),
                    "netns",
                    node_ns.as_str(),
                ],
            ),
        ));
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!(
                "attach `{}` to bridge `{}`",
                node.host_veth(&self.name),
                self.bridge_name
            ),
            CommandSpec::new(
                "ip",
                [
                    "link",
                    "set",
                    node.host_veth(&self.name).as_str(),
                    "master",
                    self.bridge_name.as_str(),
                ],
            ),
        ));
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!("bring host side `{}` up", node.host_veth(&self.name)),
            CommandSpec::new(
                "ip",
                ["link", "set", node.host_veth(&self.name).as_str(), "up"],
            ),
        ));
        plan.push(node_exec_step(
            &node_ns,
            format!("bring loopback up in node `{}`", node.name),
            ["ip", "link", "set", "lo", "up"],
        ));
        plan.push(node_exec_step(
            &node_ns,
            format!(
                "rename node uplink `{}` to `eth0`",
                node.node_veth(&self.name)
            ),
            [
                "ip",
                "link",
                "set",
                node.node_veth(&self.name).as_str(),
                "name",
                "eth0",
            ],
        ));
        plan.push(node_exec_step(
            &node_ns,
            format!(
                "assign node uplink address `{}/24`",
                self.node_uplink_ip(node)
            ),
            [
                "ip",
                "addr",
                "add",
                &format!("{}/24", self.node_uplink_ip(node)),
                "dev",
                "eth0",
            ],
        ));
        plan.push(node_exec_step(
            &node_ns,
            format!("bring node uplink up for `{}`", node.name),
            ["ip", "link", "set", "eth0", "up"],
        ));
        plan.push(node_exec_step(
            &node_ns,
            format!("set node default route through `{}`", self.bridge_addr),
            [
                "ip",
                "route",
                "add",
                "default",
                "via",
                &self.bridge_addr.to_string(),
            ],
        ));
        plan.push(node_exec_step(
            &node_ns,
            format!("enable IPv4 forwarding in node `{}`", node.name),
            ["sysctl", "-w", "net.ipv4.ip_forward=1"],
        ));
    }

    fn node_cleanup(&self, plan: &mut CommandPlan, node: &Node) {
        plan.push(PlanStep::best_effort(
            Stage::Cleanup,
            format!("delete node namespace `{}`", node.netns(&self.name)),
            CommandSpec::new("ip", ["netns", "del", node.netns(&self.name).as_str()]),
        ));
        plan.push(PlanStep::best_effort(
            Stage::Cleanup,
            format!("delete host veth `{}`", node.host_veth(&self.name)),
            CommandSpec::new("ip", ["link", "del", node.host_veth(&self.name).as_str()]),
        ));
    }

    fn pod_setup(&self, plan: &mut CommandPlan, pod: &Pod) {
        let pod_ns = pod.netns(&self.name);
        let node_ns = self
            .node_by_name(&pod.node)
            .expect("validated topology")
            .netns(&self.name);
        let address = self.pod_address(pod);
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!("create pod namespace `{pod_ns}`"),
            CommandSpec::new("ip", ["netns", "add", pod_ns.as_str()]),
        ));
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!(
                "create veth pair `{}` <-> `{}` for pod `{}`",
                pod.node_veth(&self.name),
                pod.pod_veth(&self.name),
                pod.name
            ),
            CommandSpec::new(
                "ip",
                [
                    "link",
                    "add",
                    pod.node_veth(&self.name).as_str(),
                    "type",
                    "veth",
                    "peer",
                    "name",
                    pod.pod_veth(&self.name).as_str(),
                ],
            ),
        ));
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!("move `{}` into node namespace", pod.node_veth(&self.name)),
            CommandSpec::new(
                "ip",
                [
                    "link",
                    "set",
                    pod.node_veth(&self.name).as_str(),
                    "netns",
                    node_ns.as_str(),
                ],
            ),
        ));
        plan.push(PlanStep::strict(
            Stage::Setup,
            format!("move `{}` into pod namespace", pod.pod_veth(&self.name)),
            CommandSpec::new(
                "ip",
                [
                    "link",
                    "set",
                    pod.pod_veth(&self.name).as_str(),
                    "netns",
                    pod_ns.as_str(),
                ],
            ),
        ));
        plan.push(node_exec_step(
            &node_ns,
            format!(
                "assign node-side pod link `{}` to `{}`",
                pod.node_veth(&self.name),
                address.node_side_cidr
            ),
            [
                "ip",
                "addr",
                "add",
                address.node_side_cidr.as_str(),
                "dev",
                pod.node_veth(&self.name).as_str(),
            ],
        ));
        plan.push(node_exec_step(
            &node_ns,
            format!(
                "bring node-side pod link `{}` up",
                pod.node_veth(&self.name)
            ),
            [
                "ip",
                "link",
                "set",
                pod.node_veth(&self.name).as_str(),
                "up",
            ],
        ));
        plan.push(node_exec_step(
            &pod_ns,
            format!("bring loopback up in pod `{}`", pod.name),
            ["ip", "link", "set", "lo", "up"],
        ));
        plan.push(node_exec_step(
            &pod_ns,
            format!("rename pod link `{}` to `eth0`", pod.pod_veth(&self.name)),
            [
                "ip",
                "link",
                "set",
                pod.pod_veth(&self.name).as_str(),
                "name",
                "eth0",
            ],
        ));
        plan.push(node_exec_step(
            &pod_ns,
            format!("assign pod address `{}`", address.pod_cidr),
            [
                "ip",
                "addr",
                "add",
                address.pod_cidr.as_str(),
                "dev",
                "eth0",
            ],
        ));
        plan.push(node_exec_step(
            &pod_ns,
            format!("bring pod link up for `{}`", pod.name),
            ["ip", "link", "set", "eth0", "up"],
        ));
        plan.push(node_exec_step(
            &pod_ns,
            format!("route pod `{}` through its node-side peer", pod.name),
            [
                "ip",
                "route",
                "add",
                "default",
                "via",
                &address.node_side_ip.to_string(),
            ],
        ));
    }

    fn pod_cleanup(&self, plan: &mut CommandPlan, pod: &Pod) {
        plan.push(PlanStep::best_effort(
            Stage::Cleanup,
            format!("delete pod namespace `{}`", pod.netns(&self.name)),
            CommandSpec::new("ip", ["netns", "del", pod.netns(&self.name).as_str()]),
        ));
    }

    fn cross_node_routes(&self, plan: &mut CommandPlan) {
        for node in &self.nodes {
            let node_ns = node.netns(&self.name);
            for pod in &self.pods {
                if pod.node == node.name {
                    continue;
                }
                let remote_node = self.node_by_name(&pod.node).expect("validated topology");
                let pod_ip = self.pod_address(pod).pod_ip;
                plan.push(node_exec_step(
                    &node_ns,
                    format!(
                        "route remote pod `{}` through node `{}`",
                        pod.name, remote_node.name
                    ),
                    [
                        "ip",
                        "route",
                        "add",
                        &format!("{pod_ip}/32"),
                        "via",
                        &self.node_uplink_ip(remote_node).to_string(),
                    ],
                ));
            }
        }
    }

    fn node_by_name(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.name == name)
    }

    fn pod_by_name(&self, name: &str) -> Option<&Pod> {
        self.pods.iter().find(|pod| pod.name == name)
    }

    fn node_uplink_ip(&self, node: &Node) -> Ipv4Addr {
        Ipv4Addr::new(10, 200, 0, 10 + node.index as u8)
    }

    fn pod_address(&self, pod: &Pod) -> PodAddress {
        let subnet = pod.index + 1;
        let node_side_ip = Ipv4Addr::new(10, 244, subnet as u8, 1);
        let pod_ip = Ipv4Addr::new(10, 244, subnet as u8, 2);
        PodAddress {
            pod_ip,
            pod_cidr: format!("{pod_ip}/30"),
            node_side_ip,
            node_side_cidr: format!("{node_side_ip}/30"),
        }
    }
}

impl TopologyBuilder {
    pub fn bridge_name(mut self, name: impl Into<String>) -> Self {
        self.bridge_name = name.into();
        self
    }

    pub fn add_node(mut self, name: impl Into<String>) -> Self {
        self.nodes.push(PendingNode {
            name: name.into(),
            index: None,
        });
        self
    }

    pub fn add_node_with_index(mut self, name: impl Into<String>, index: usize) -> Self {
        self.nodes.push(PendingNode {
            name: name.into(),
            index: Some(index),
        });
        self
    }

    pub fn add_pod(mut self, name: impl Into<String>, node: impl Into<String>) -> Self {
        self.pods.push(PendingPod {
            name: name.into(),
            node: node.into(),
            index: None,
            workload: None,
        });
        self
    }

    pub fn add_pod_with_index(
        mut self,
        name: impl Into<String>,
        node: impl Into<String>,
        index: usize,
    ) -> Self {
        self.pods.push(PendingPod {
            name: name.into(),
            node: node.into(),
            index: Some(index),
            workload: None,
        });
        self
    }

    pub fn add_workload_pod(
        mut self,
        name: impl Into<String>,
        node: impl Into<String>,
        command: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.pods.push(PendingPod {
            name: name.into(),
            node: node.into(),
            index: None,
            workload: Some(command.into_iter().map(Into::into).collect()),
        });
        self
    }

    pub fn add_workload_pod_with_index(
        mut self,
        name: impl Into<String>,
        node: impl Into<String>,
        index: usize,
        command: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.pods.push(PendingPod {
            name: name.into(),
            node: node.into(),
            index: Some(index),
            workload: Some(command.into_iter().map(Into::into).collect()),
        });
        self
    }

    pub fn build(self) -> Result<Topology> {
        validate_name("facility", &self.name)?;
        validate_name("bridge", &self.bridge_name)?;
        if self.nodes.len() > 240 {
            return Err(FacilityError::TooManyItems {
                kind: "nodes",
                max: 240,
                actual: self.nodes.len(),
            });
        }
        if self.pods.len() > 240 {
            return Err(FacilityError::TooManyItems {
                kind: "pods",
                max: 240,
                actual: self.pods.len(),
            });
        }

        let mut node_names = BTreeSet::new();
        let mut node_indexes = BTreeSet::new();
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for (fallback_index, node) in self.nodes.into_iter().enumerate() {
            let name = node.name;
            validate_name("node", &name)?;
            if !node_names.insert(name.clone()) {
                return Err(FacilityError::DuplicateName { kind: "node", name });
            }
            let index = node.index.unwrap_or(fallback_index);
            if index > 239 {
                return Err(FacilityError::TooManyItems {
                    kind: "node index",
                    max: 239,
                    actual: index,
                });
            }
            if !node_indexes.insert(index) {
                return Err(FacilityError::DuplicateName {
                    kind: "node index",
                    name: index.to_string(),
                });
            }
            nodes.push(Node { name, index });
        }

        let known_nodes = nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<BTreeSet<_>>();
        let mut pod_names = BTreeSet::new();
        let mut pods = Vec::with_capacity(self.pods.len());
        let mut pod_indexes = BTreeSet::new();
        for (fallback_index, pod) in self.pods.into_iter().enumerate() {
            validate_name("pod", &pod.name)?;
            if !pod_names.insert(pod.name.clone()) {
                return Err(FacilityError::DuplicateName {
                    kind: "pod",
                    name: pod.name,
                });
            }
            if !known_nodes.contains(pod.node.as_str()) {
                return Err(FacilityError::UnknownNode {
                    pod: pod.name,
                    node: pod.node,
                });
            }
            let index = pod.index.unwrap_or(fallback_index);
            if index > 239 {
                return Err(FacilityError::TooManyItems {
                    kind: "pod index",
                    max: 239,
                    actual: index,
                });
            }
            if !pod_indexes.insert(index) {
                return Err(FacilityError::DuplicateName {
                    kind: "pod index",
                    name: index.to_string(),
                });
            }
            pods.push(Pod {
                name: pod.name,
                node: pod.node,
                index,
                workload: pod.workload,
            });
        }

        Ok(Topology {
            name: self.name,
            bridge_name: self.bridge_name,
            bridge_addr: self.bridge_addr,
            nodes,
            pods,
        })
    }
}

impl Node {
    pub fn name(&self) -> &str {
        &self.name
    }

    fn netns(&self, facility: &str) -> String {
        format!("vf-{facility}-node-{}", self.name)
    }

    fn host_veth(&self, facility: &str) -> String {
        format!("{}h{}", link_prefix(facility), self.index)
    }

    fn node_veth(&self, facility: &str) -> String {
        format!("{}n{}", link_prefix(facility), self.index)
    }
}

impl Pod {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn node(&self) -> &str {
        &self.node
    }

    fn netns(&self, facility: &str) -> String {
        format!("vf-{facility}-pod-{}", self.name)
    }

    fn node_veth(&self, facility: &str) -> String {
        format!("{}pn{}", link_prefix(facility), self.index)
    }

    fn pod_veth(&self, facility: &str) -> String {
        format!("{}pp{}", link_prefix(facility), self.index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportReport {
    pub os: &'static str,
    pub has_ip_command: bool,
    pub has_mount_command: bool,
    pub has_unshare_command: bool,
    pub has_proc_netns: bool,
    pub has_uid_map: bool,
    pub unprivileged_userns_restricted: Option<bool>,
}

impl SupportReport {
    pub fn is_ready_for_execution(&self) -> bool {
        self.os == "linux" && self.has_ip_command && self.has_proc_netns
    }

    pub fn is_ready_for_rootless_bootstrap(&self) -> bool {
        self.is_ready_for_execution()
            && self.has_mount_command
            && self.has_unshare_command
            && self.has_uid_map
            && self.unprivileged_userns_restricted != Some(true)
    }

    pub fn render(&self) -> String {
        format!(
            "os: {}\nip command: {}\nmount command: {}\nunshare command: {}\n/proc/self/ns/net: {}\n/proc/self/uid_map: {}\nunprivileged userns restricted: {}\nready for namespace execution: {}\nready for unprivileged rootless bootstrap: {}",
            self.os,
            yes_no(self.has_ip_command),
            yes_no(self.has_mount_command),
            yes_no(self.has_unshare_command),
            yes_no(self.has_proc_netns),
            yes_no(self.has_uid_map),
            option_yes_no(self.unprivileged_userns_restricted),
            yes_no(self.is_ready_for_execution()),
            yes_no(self.is_ready_for_rootless_bootstrap())
        )
    }
}

pub fn support_report() -> SupportReport {
    SupportReport {
        os: env::consts::OS,
        has_ip_command: command_exists("ip"),
        has_mount_command: command_exists("mount"),
        has_unshare_command: command_exists("unshare"),
        has_proc_netns: fs::metadata("/proc/self/ns/net").is_ok(),
        has_uid_map: fs::metadata("/proc/self/uid_map").is_ok(),
        unprivileged_userns_restricted: read_bool_sysctl(
            "/proc/sys/kernel/apparmor_restrict_unprivileged_userns",
        ),
    }
}

pub fn ensure_linux_namespace_support() -> Result<()> {
    let report = support_report();
    if report.os != "linux" {
        return Err(FacilityError::UnsupportedPlatform { current: report.os });
    }
    if !report.has_ip_command {
        return Err(FacilityError::MissingCommand { command: "ip" });
    }
    if !report.has_proc_netns {
        return Err(FacilityError::UnsupportedPlatform { current: report.os });
    }
    Ok(())
}

pub fn ensure_rootless_bootstrap_support() -> Result<()> {
    ensure_linux_namespace_support()?;
    let report = support_report();
    if !report.has_mount_command {
        return Err(FacilityError::MissingCommand { command: "mount" });
    }
    if !report.has_unshare_command {
        return Err(FacilityError::MissingCommand { command: "unshare" });
    }
    if !report.has_uid_map {
        return Err(FacilityError::UnsupportedPlatform { current: report.os });
    }
    Ok(())
}

pub fn apply_plan(plan: &CommandPlan) -> Result<()> {
    ensure_linux_namespace_support()?;
    apply_plan_steps(plan)
}

pub fn apply_rootless_bootstrap_plan(plan: &CommandPlan) -> Result<()> {
    ensure_rootless_bootstrap_support()?;
    apply_plan_steps(plan)
}

fn apply_plan_steps(plan: &CommandPlan) -> Result<()> {
    for step in plan.steps() {
        let output = Command::new(step.command.program())
            .args(step.command.args())
            .output()
            .map_err(|err| FacilityError::CommandFailed {
                command: step.command.as_shell(),
                code: None,
                stderr: err.to_string(),
            })?;
        if !output.status.success() && !step.allow_failure {
            return Err(FacilityError::CommandFailed {
                command: step.command.as_shell(),
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn run_in_network_namespace<F, T>(netns_path: impl AsRef<Path>, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    ensure_linux_namespace_support()?;
    let netns_path = netns_path.as_ref().to_path_buf();
    let thread_name = netns_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("vf-netns-{name}"))
        .unwrap_or_else(|| "vf-netns".to_string());
    let handle = thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let netns = File::open(&netns_path).map_err(|err| FacilityError::NamespaceSyscall {
                syscall: "open",
                detail: format!("{}: {err}", netns_path.display()),
            })?;
            let rc = unsafe { libc::setns(netns.as_raw_fd(), libc::CLONE_NEWNET) };
            if rc != 0 {
                return Err(FacilityError::NamespaceSyscall {
                    syscall: "setns",
                    detail: std::io::Error::last_os_error().to_string(),
                });
            }
            f()
        })
        .map_err(|err| FacilityError::NamespaceSyscall {
            syscall: "thread::spawn",
            detail: err.to_string(),
        })?;
    handle.join().map_err(|_| FacilityError::ThreadPanicked)?
}

#[cfg(not(target_os = "linux"))]
pub fn run_in_network_namespace<F, T>(_netns_path: impl AsRef<Path>, _f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    Err(FacilityError::UnsupportedPlatform {
        current: env::consts::OS,
    })
}

pub fn resolver_table(topology: &Topology) -> BTreeMap<String, Ipv4Addr> {
    topology
        .pods()
        .iter()
        .map(|pod| {
            (
                pod.name().to_string(),
                topology
                    .resolve(pod.name())
                    .expect("pod came from topology"),
            )
        })
        .collect()
}

fn node_exec_step<const N: usize>(
    netns: &str,
    description: impl Into<String>,
    args: [&str; N],
) -> PlanStep {
    let mut command_args = vec!["netns".to_string(), "exec".to_string(), netns.to_string()];
    command_args.extend(args.into_iter().map(str::to_string));
    PlanStep::strict(
        Stage::Setup,
        description,
        CommandSpec::new("ip", command_args),
    )
}

fn validate_name(kind: &'static str, name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(FacilityError::InvalidName {
            kind,
            name: name.to_string(),
            reason: "must not be empty",
        });
    }
    if name.len() > 40 {
        return Err(FacilityError::InvalidName {
            kind,
            name: name.to_string(),
            reason: "must be 40 characters or fewer",
        });
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(FacilityError::InvalidName {
            kind,
            name: name.to_string(),
            reason: "use only ASCII letters, digits, hyphen, or underscore",
        });
    }
    Ok(())
}

fn link_prefix(facility: &str) -> String {
    let suffix = facility
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .take(6)
        .map(char::from)
        .collect::<String>()
        .to_ascii_lowercase();
    if suffix.is_empty() {
        "vf".to_string()
    } else {
        format!("vf{suffix}")
    }
}

fn command_exists(command: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|path| path.join(command).is_file())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn read_bool_sysctl(path: &str) -> Option<bool> {
    let value = fs::read_to_string(path).ok()?;
    match value.trim() {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn shell_quote(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':' | b'=')
    }) {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn option_yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_topology_matches_transcript_shape() {
        let topology = Topology::smoke().unwrap();

        assert_eq!(topology.nodes().len(), 1);
        assert_eq!(
            topology.pods().iter().map(Pod::name).collect::<Vec<_>>(),
            vec!["proxy", "server", "client"]
        );
        assert_eq!(
            topology.resolve("proxy"),
            Some(Ipv4Addr::new(10, 244, 1, 2))
        );
        assert_eq!(
            topology.resolve("server"),
            Some(Ipv4Addr::new(10, 244, 2, 2))
        );
        assert_eq!(
            topology.resolve("client"),
            Some(Ipv4Addr::new(10, 244, 3, 2))
        );
    }

    #[test]
    fn setup_plan_contains_namespace_veth_bridge_and_routes() {
        let topology = Topology::smoke().unwrap();
        let rendered = topology.setup_plan().render_shell();

        assert!(rendered.contains("ip link add vf-br0 type bridge"));
        assert!(rendered.contains("ip netns add vf-smoke-node-default-node"));
        assert!(rendered.contains("ip link add vfsmokeh0 type veth peer name vfsmoken0"));
        assert!(rendered.contains("ip netns add vf-smoke-pod-server"));
        assert!(rendered
            .contains("ip netns exec vf-smoke-pod-server ip route add default via 10.244.2.1"));
    }

    #[test]
    fn workload_plan_runs_inside_pod_namespace() {
        let topology = Topology::builder("with-workload")
            .add_node("default-node")
            .add_workload_pod(
                "server",
                "default-node",
                ["python3", "-m", "http.server", "8080"],
            )
            .build()
            .unwrap();

        let rendered = topology.workload_plan().unwrap().render_shell();

        assert!(rendered
            .contains("ip netns exec vf-with-workload-pod-server python3 -m http.server 8080"));
    }

    #[test]
    fn ping_plan_checks_pod_to_pod_connectivity() {
        let topology = Topology::smoke().unwrap();
        let rendered = topology
            .ping_plan("client", "server")
            .unwrap()
            .render_shell();

        assert!(rendered.contains("ip netns exec vf-smoke-pod-client ping -c 1 10.244.2.2"));
    }

    #[test]
    fn pod_namespace_is_exposed_for_setns_runtime_tests() {
        let topology = Topology::smoke().unwrap();

        assert_eq!(
            topology.pod_namespace("client").unwrap(),
            "vf-smoke-pod-client"
        );
    }

    #[test]
    fn rootless_bootstrap_plan_wraps_inner_command() {
        let plan = RootlessBootstrap::new("/tmp/vf")
            .with_standard_mounts()
            .bootstrap_plan(CommandSpec::new("cargo", ["test"]))
            .unwrap();
        let rendered = plan.render_shell();

        assert!(rendered.contains("mkdir -p /tmp/vf"));
        assert!(rendered.contains("touch /tmp/vf/run/xtables.lock"));
        assert!(rendered.contains("mkdir -p /tmp/vf/run/netns"));
        assert!(rendered.contains("mkdir -p /tmp/vf/run"));
        assert!(
            rendered.contains("unshare --user --map-root-user --mount --net --fork -- sh -eu -c")
        );
        assert!(rendered.contains("mount --bind"));
        assert!(rendered.contains("/tmp/vf/run /run rw dir -- cargo test"));
    }

    #[test]
    fn support_report_distinguishes_namespace_and_rootless_readiness() {
        let namespace_only = SupportReport {
            os: "linux",
            has_ip_command: true,
            has_mount_command: false,
            has_unshare_command: false,
            has_proc_netns: true,
            has_uid_map: false,
            unprivileged_userns_restricted: None,
        };
        let rootless = SupportReport {
            has_mount_command: true,
            has_unshare_command: true,
            has_uid_map: true,
            ..namespace_only.clone()
        };

        assert!(namespace_only.is_ready_for_execution());
        assert!(!namespace_only.is_ready_for_rootless_bootstrap());
        assert!(rootless.is_ready_for_rootless_bootstrap());

        let restricted = SupportReport {
            unprivileged_userns_restricted: Some(true),
            ..rootless
        };
        assert!(!restricted.is_ready_for_rootless_bootstrap());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn namespace_thread_reports_unsupported_off_linux() {
        let err = run_in_network_namespace("/tmp/missing", || Ok::<_, FacilityError>(()))
            .expect_err("non-Linux should not run namespace closures");

        assert_eq!(
            err,
            FacilityError::UnsupportedPlatform {
                current: env::consts::OS
            }
        );
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn rootless_bootstrap_execution_reports_unsupported_off_linux() {
        let plan = RootlessBootstrap::new("/tmp/vf")
            .bootstrap_plan(CommandSpec::new("cargo", ["test"]))
            .unwrap();
        let err = apply_rootless_bootstrap_plan(&plan)
            .expect_err("non-Linux should not execute rootless bootstrap");

        assert_eq!(
            err,
            FacilityError::UnsupportedPlatform {
                current: env::consts::OS
            }
        );
    }

    #[test]
    fn cleanup_plan_is_best_effort() {
        let topology = Topology::smoke().unwrap();
        let cleanup = topology.cleanup_plan();

        assert!(cleanup.steps().iter().all(|step| step.allow_failure));
        assert!(cleanup
            .render_shell()
            .contains("ip link del vf-br0 || true"));
    }

    #[test]
    fn duplicate_pod_names_are_rejected() {
        let err = Topology::builder("bad")
            .add_node("default-node")
            .add_pod("server", "default-node")
            .add_pod("server", "default-node")
            .build()
            .unwrap_err();

        assert_eq!(
            err,
            FacilityError::DuplicateName {
                kind: "pod",
                name: "server".to_string()
            }
        );
    }

    #[test]
    fn unknown_nodes_are_rejected() {
        let err = Topology::builder("bad")
            .add_node("default-node")
            .add_pod("server", "missing-node")
            .build()
            .unwrap_err();

        assert_eq!(
            err,
            FacilityError::UnknownNode {
                pod: "server".to_string(),
                node: "missing-node".to_string()
            }
        );
    }

    #[test]
    fn shell_rendering_quotes_unsafe_arguments() {
        let command = CommandSpec::new("echo", ["plain", "two words", "name='quoted'"]);

        assert_eq!(
            command.as_shell(),
            "echo plain 'two words' 'name='\\''quoted'\\'''"
        );
    }
}
