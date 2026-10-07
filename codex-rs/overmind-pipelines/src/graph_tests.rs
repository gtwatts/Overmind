use pretty_assertions::assert_eq;

use super::*;

#[test]
fn keeps_declared_order_when_possible() {
    let deps = vec![vec![], vec![0], vec![1]];
    assert_eq!(topological_order(&deps), Ok(vec![0, 1, 2]));
}

#[test]
fn orders_a_dag_stably() {
    // 0 and 2 are roots; 1 needs 2; 3 needs 0 and 1.
    let deps = vec![vec![], vec![2], vec![], vec![0, 1]];
    assert_eq!(topological_order(&deps), Ok(vec![0, 2, 1, 3]));
}

#[test]
fn reports_cycles() {
    let deps = vec![vec![], vec![2], vec![1]];
    assert_eq!(topological_order(&deps), Err(vec![1, 2]));
}

#[test]
fn downstream_is_transitive() {
    let deps = vec![vec![], vec![0], vec![1], vec![]];
    assert_eq!(
        downstream(&deps, 0).into_iter().collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(downstream(&deps, 3).is_empty());
}
