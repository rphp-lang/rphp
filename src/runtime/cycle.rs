//! Explicit request-local cycle collection.
//!
//! Possible roots are weakly recorded by `Value` when a shared cycle-capable
//! allocation loses a handle. An explicit pass expands only those roots and
//! live weak-object sidecars into a graph, subtracts internal ownership from
//! Rc counts, and treats WeakMap values as ephemerons rather than ordinary
//! strong edges.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::value::{CycleNodeKind, Value, begin_cycle_collection, cycle_root_snapshot};
use crate::vm::execute::{VmError, run_cycle_object_destructor};

use super::ExecutorGlobals;

struct CycleNode {
    identity: usize,
    kind: CycleNodeKind,
    value: Value,
}

struct EphemeronEdge {
    map: usize,
    key: usize,
    value: usize,
}

#[derive(Default)]
struct CycleGraph {
    nodes: Vec<CycleNode>,
    indices: HashMap<usize, usize>,
    ordinary_edges: Vec<(usize, usize)>,
    ephemerons: Vec<EphemeronEdge>,
    stale_weak_identities: Vec<usize>,
}

impl CycleGraph {
    fn add_node(&mut self, value: Value) -> Option<usize> {
        let (identity, kind) = value.cycle_node()?;
        if self.indices.contains_key(&identity) {
            return Some(identity);
        }
        self.indices.insert(identity, self.nodes.len());
        self.nodes.push(CycleNode {
            identity,
            kind,
            value,
        });
        Some(identity)
    }

    fn expand_value_edges(&mut self) {
        let mut position = 0;
        while position < self.nodes.len() {
            let source = self.nodes[position].identity;
            let children = self.nodes[position].value.cycle_child_handles();
            for child in children {
                let Some((target, _)) = child.cycle_node() else {
                    continue;
                };
                self.add_node(child);
                self.ordinary_edges.push((source, target));
            }
            position += 1;
        }
    }

    fn live_identities(&self) -> HashSet<usize> {
        let mut incoming = vec![0usize; self.nodes.len()];
        for &(_, target) in &self.ordinary_edges {
            if let Some(&target) = self.indices.get(&target) {
                incoming[target] = incoming[target].saturating_add(1);
            }
        }
        for edge in &self.ephemerons {
            if let Some(&target) = self.indices.get(&edge.value) {
                incoming[target] = incoming[target].saturating_add(1);
            }
        }

        let mut adjacency = vec![Vec::new(); self.nodes.len()];
        for &(source, target) in &self.ordinary_edges {
            if let (Some(&source), Some(&target)) =
                (self.indices.get(&source), self.indices.get(&target))
            {
                adjacency[source].push(target);
            }
        }

        let mut live = vec![false; self.nodes.len()];
        let mut queue = VecDeque::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let strong = node
                .value
                .cycle_strong_count()
                .expect("cycle graph nodes retain one Rc owner");
            // One owner belongs to this graph snapshot. Every recorded
            // incoming edge corresponds to one real Value handle in another
            // graph node or weak-object sidecar.
            if strong.saturating_sub(1 + incoming[index]) != 0 {
                live[index] = true;
                queue.push_back(index);
            }
        }
        propagate(&adjacency, &mut live, &mut queue);

        loop {
            let mut changed = false;
            for edge in &self.ephemerons {
                let (Some(&map), Some(&key), Some(&value)) = (
                    self.indices.get(&edge.map),
                    self.indices.get(&edge.key),
                    self.indices.get(&edge.value),
                ) else {
                    continue;
                };
                if live[map] && live[key] && !live[value] {
                    live[value] = true;
                    queue.push_back(value);
                    changed = true;
                }
            }
            propagate(&adjacency, &mut live, &mut queue);
            if !changed {
                break;
            }
        }

        self.nodes
            .iter()
            .zip(live)
            .filter_map(|(node, live)| live.then_some(node.identity))
            .collect()
    }

    /// Nodes in strongly connected garbage components determine PHP's
    /// returned collection count. Acyclic children are reclaimed by breaking
    /// their owner's edges and receive destructors, but are not counted.
    fn cyclic_identities(&self, garbage: &HashSet<usize>) -> HashSet<usize> {
        let mut adjacency = vec![Vec::new(); self.nodes.len()];
        let mut reverse = vec![Vec::new(); self.nodes.len()];
        let mut add = |source: usize, target: usize| {
            let (Some(&source), Some(&target)) =
                (self.indices.get(&source), self.indices.get(&target))
            else {
                return;
            };
            if garbage.contains(&self.nodes[source].identity)
                && garbage.contains(&self.nodes[target].identity)
            {
                adjacency[source].push(target);
                reverse[target].push(source);
            }
        };
        for &(source, target) in &self.ordinary_edges {
            add(source, target);
        }
        // For SCC membership an ephemeron is rooted at its key: the value is
        // retained only while that key is reachable. This exposes key/value
        // cycles without turning the weak key into an ordinary map edge.
        for edge in &self.ephemerons {
            add(edge.key, edge.value);
        }

        let mut seen = vec![false; self.nodes.len()];
        let mut order = Vec::with_capacity(self.nodes.len());
        for start in 0..self.nodes.len() {
            if seen[start] || !garbage.contains(&self.nodes[start].identity) {
                continue;
            }
            seen[start] = true;
            let mut stack = vec![(start, 0usize)];
            while let Some((node, edge)) = stack.last_mut() {
                if let Some(&target) = adjacency[*node].get(*edge) {
                    *edge += 1;
                    if !seen[target] {
                        seen[target] = true;
                        stack.push((target, 0));
                    }
                } else {
                    order.push(*node);
                    stack.pop();
                }
            }
        }

        let mut component = vec![usize::MAX; self.nodes.len()];
        let mut components = Vec::<Vec<usize>>::new();
        for &start in order.iter().rev() {
            if component[start] != usize::MAX {
                continue;
            }
            let id = components.len();
            let mut members = Vec::new();
            let mut stack = vec![start];
            component[start] = id;
            while let Some(node) = stack.pop() {
                members.push(node);
                for &source in &reverse[node] {
                    if component[source] == usize::MAX {
                        component[source] = id;
                        stack.push(source);
                    }
                }
            }
            components.push(members);
        }

        let mut cyclic = HashSet::new();
        for members in components {
            let has_cycle = members.len() > 1
                || members
                    .first()
                    .is_some_and(|&node| adjacency[node].iter().any(|target| *target == node));
            if has_cycle {
                cyclic.extend(members.into_iter().map(|node| self.nodes[node].identity));
            }
        }
        cyclic
    }

    /// Order destructor dispatch for values retained by a suspended generator
    /// from the generator's saved operand graph. Those operands may have been
    /// entered in the possible-root buffer earlier by Rust-side temporary
    /// clones, but Zend owns them through the activation and releases pending
    /// call operands before local CVs.
    fn destructor_order(&self, garbage: &HashSet<usize>) -> Vec<usize> {
        let mut adjacency = vec![Vec::new(); self.nodes.len()];
        for &(source, target) in &self.ordinary_edges {
            if let (Some(&source), Some(&target)) =
                (self.indices.get(&source), self.indices.get(&target))
                && garbage.contains(&self.nodes[source].identity)
                && garbage.contains(&self.nodes[target].identity)
            {
                adjacency[source].push(target);
            }
        }

        let mut ordered = Vec::with_capacity(self.nodes.len());
        let mut visited = vec![false; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            let is_generator = node.kind == CycleNodeKind::Object
                && node
                    .value
                    .as_object()
                    .is_some_and(|object| object.generator.is_some());
            if !is_generator || !garbage.contains(&node.identity) {
                continue;
            }
            let mut stack = vec![index];
            while let Some(current) = stack.pop() {
                if visited[current] || !garbage.contains(&self.nodes[current].identity) {
                    continue;
                }
                visited[current] = true;
                ordered.push(current);
                stack.extend(adjacency[current].iter().rev().copied());
            }
        }
        ordered.extend(self.nodes.iter().enumerate().filter_map(|(index, node)| {
            (garbage.contains(&node.identity) && !visited[index]).then_some(index)
        }));
        ordered
    }
}

fn propagate(adjacency: &[Vec<usize>], live: &mut [bool], queue: &mut VecDeque<usize>) {
    while let Some(source) = queue.pop_front() {
        for &target in &adjacency[source] {
            if !live[target] {
                live[target] = true;
                queue.push_back(target);
            }
        }
    }
}

impl ExecutorGlobals {
    fn build_cycle_graph(&self) -> CycleGraph {
        let mut graph = CycleGraph::default();
        for root in cycle_root_snapshot() {
            graph.add_node(root);
        }

        let weak = self.weak_cycle_snapshot();
        graph.stale_weak_identities = weak.stale_identities;
        for map in weak.maps {
            if let Some(owner) = map.map {
                graph.add_node(owner);
            }
            for entry in map.entries {
                if let Some(key) = entry.key {
                    graph.add_node(key);
                }
                let value = entry
                    .value
                    .cycle_node()
                    .expect("WeakMap sidecar values are cycle-capable")
                    .0;
                graph.add_node(entry.value);
                graph.ephemerons.push(EphemeronEdge {
                    map: map.map_identity,
                    key: entry.key_identity,
                    value,
                });
            }
        }
        for iterator in weak.iterators {
            if let Some(owner) = iterator.iterator {
                graph.add_node(owner);
            }
            let map = iterator
                .map
                .cycle_node()
                .expect("WeakMap iterators retain their map object")
                .0;
            graph.add_node(iterator.map);
            graph.ordinary_edges.push((iterator.iterator_identity, map));
        }
        graph.expand_value_edges();
        graph
    }

    pub(crate) fn collect_cycles(&mut self) -> Result<usize, VmError> {
        let Some(mut guard) = begin_cycle_collection() else {
            return Ok(0);
        };

        let collector_started = Instant::now();
        let mut initial = self.build_cycle_graph();
        let ran = !initial.nodes.is_empty();
        if ran {
            guard.mark_ran();
        }
        let initially_live = initial.live_identities();
        let garbage: Vec<(usize, CycleNodeKind)> = initial
            .nodes
            .iter()
            .filter(|node| !initially_live.contains(&node.identity))
            .map(|node| (node.identity, node.kind))
            .collect();
        let garbage_identities: HashSet<usize> =
            garbage.iter().map(|(identity, _)| *identity).collect();
        let cyclic = initial.cyclic_identities(&garbage_identities);

        let destructor_order = initial.destructor_order(&garbage_identities);
        let has_destructors = destructor_order
            .iter()
            .any(|index| initial.nodes[*index].kind == CycleNodeKind::Object);
        let mut collector_time = Duration::ZERO;
        let mut destructor_time = Duration::ZERO;
        let collector_resumed = if has_destructors {
            let destructor_started = Instant::now();
            collector_time = destructor_started.duration_since(collector_started);
            for index in destructor_order {
                let node = &initial.nodes[index];
                if node.kind == CycleNodeKind::Object {
                    run_cycle_object_destructor(self, &node.value)?;
                    if self.exception.is_some() {
                        return Ok(0);
                    }
                }
            }
            let resumed = Instant::now();
            destructor_time = resumed.duration_since(destructor_started);
            resumed
        } else {
            collector_started
        };
        let mut stale = std::mem::take(&mut initial.stale_weak_identities);
        drop(initial);

        // Destructors may create roots, remove edges or resurrect a complete
        // component. Rebuild from current ownership before releasing anything.
        let current = self.build_cycle_graph();
        let currently_live = current.live_identities();
        stale.extend(current.stale_weak_identities.iter().copied());
        let collected: HashSet<usize> = garbage
            .iter()
            .filter_map(|(identity, _)| (!currently_live.contains(identity)).then_some(*identity))
            .collect();

        stale.extend(collected.iter().copied());
        stale.sort_unstable();
        stale.dedup();
        let free_started = Instant::now();
        collector_time =
            collector_time.saturating_add(free_started.duration_since(collector_resumed));
        let mut released = Vec::new();
        for identity in stale {
            released.extend(self.release_weak_object(identity));
        }
        drop(released);

        for node in &current.nodes {
            if collected.contains(&node.identity) {
                node.value.clear_cycle_edges();
            }
        }

        let count = garbage
            .iter()
            .filter(|(identity, kind)| {
                collected.contains(identity)
                    && cyclic.contains(identity)
                    && matches!(
                        kind,
                        CycleNodeKind::Array | CycleNodeKind::Object | CycleNodeKind::Closure
                    )
            })
            .count();
        drop(current);
        let free_time = Instant::now().duration_since(free_started);
        guard.complete(
            count,
            if ran { collector_time } else { Duration::ZERO },
            if ran { destructor_time } else { Duration::ZERO },
            if ran { free_time } else { Duration::ZERO },
        );
        Ok(count)
    }
}
