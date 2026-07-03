# virtualfacility

Simulate Kubernetes-style networks, nodes, and pods locally with Linux network namespaces, veth pairs, bridges, and routes.

## Comparison

| Goal | Kubernetes | virtualfacility |
| --- | --- | --- |
| Prepare a test network | `kind create cluster --name lab1` or use an existing cluster | `cargo run -- create --network vf-lab1` |
| Prepare a node | `kind create cluster --config kind.yaml` or `kubeadm join ...` | `cargo run -- create node node-1 --network vf-lab1` |
| Create a pod | `kubectl apply -f pod.yaml` | `cargo run -- create pod app-1 --node node-1 --network vf-lab1` |
| Inspect nodes | `kubectl get nodes -o wide` | `cargo run -- status` |
| Inspect pods | `kubectl get pods -o wide` | `cargo run -- status` |
| Delete a pod | `kubectl delete pod app-1` | `cargo run -- delete pod <pod-id>` |
| Delete a node | `kubectl delete node node-1` | `cargo run -- delete node <node-id>` |
| Delete the test network | `kind delete cluster --name lab1` | `cargo run -- delete --network vf-lab1` |

## Create

```shell
CARGO_TARGET_DIR=/tmp/vf-target cargo run -- create --network vf-lab1
CARGO_TARGET_DIR=/tmp/vf-target cargo run -- create node node-1 --network vf-lab1
CARGO_TARGET_DIR=/tmp/vf-target cargo run -- create pod app-1 --node node-1 --network vf-lab1
CARGO_TARGET_DIR=/tmp/vf-target cargo run -- create pod app-2 --node node-1 --network vf-lab1
```

## Inspect

```shell
CARGO_TARGET_DIR=/tmp/vf-target cargo run -- status
```

Example output:

```text

NETWORKS
NAME            BRIDGE-IP           STATUS
vf-lab1         10.200.1.1/24       present

NODES
ID                        NETWORK         INTERNAL-IP         STATUS
node-1-xxxxxxxx           vf-lab1         10.200.1.10/24      present

PODS
ID                        NETWORK         NODE            IP                  STATUS
app-1-xxxxxxxx            vf-lab1         node-1          10.244.4.2/30       present
app-2-xxxxxxxx            vf-lab1         node-1          10.244.5.2/30       present
```

## Delete

Delete pods first, then nodes, then the network:

```shell
CARGO_TARGET_DIR=/tmp/vf-target cargo run -- delete pod app-1-xxxxxxxx
CARGO_TARGET_DIR=/tmp/vf-target cargo run -- delete pod app-2-xxxxxxxx
CARGO_TARGET_DIR=/tmp/vf-target cargo run -- delete node node-1-xxxxxxxx
CARGO_TARGET_DIR=/tmp/vf-target cargo run -- delete --network vf-lab1
```
