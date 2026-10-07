//! Stage ordering over resolved dependencies (step indices).

use std::collections::BTreeSet;

/// Stable topological order: Kahn's algorithm that always takes the lowest declared index
/// among the ready steps, so a linear pipeline keeps its declared order. On a cycle, returns
/// the steps that could not be ordered.
pub fn topological_order(dependencies: &[Vec<usize>]) -> Result<Vec<usize>, Vec<usize>> {
    let count = dependencies.len();
    let mut remaining: Vec<usize> = dependencies.iter().map(Vec::len).collect();
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, deps) in dependencies.iter().enumerate() {
        for dep in deps {
            if let Some(list) = dependents.get_mut(*dep) {
                list.push(index);
            }
        }
    }
    let mut ready: BTreeSet<usize> = (0..count).filter(|index| remaining[*index] == 0).collect();
    let mut order = Vec::with_capacity(count);
    while let Some(next) = ready.pop_first() {
        order.push(next);
        for dependent in &dependents[next] {
            remaining[*dependent] -= 1;
            if remaining[*dependent] == 0 {
                ready.insert(*dependent);
            }
        }
    }
    if order.len() == count {
        Ok(order)
    } else {
        Err((0..count).filter(|index| remaining[*index] > 0).collect())
    }
}

/// Every step that depends on `start`, directly or transitively (excluding `start`).
pub fn downstream(dependencies: &[Vec<usize>], start: usize) -> BTreeSet<usize> {
    let mut affected = BTreeSet::new();
    let mut frontier = vec![start];
    while let Some(current) = frontier.pop() {
        for (index, deps) in dependencies.iter().enumerate() {
            if deps.contains(&current) && affected.insert(index) {
                frontier.push(index);
            }
        }
    }
    affected
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
