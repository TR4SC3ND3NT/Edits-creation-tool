use edits_core::{check, describe, Evaluator, Project};

#[test]
fn example_project_loads_checks_and_evaluates() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/beat-loop.json")).unwrap();
    let p = Project::from_json(&text).unwrap();
    let findings = check::check(&p, None);
    assert!(!check::has_errors(&findings), "{findings:#?}");
    // Round-trips through JSON.
    let again = Project::from_json(&p.to_json()).unwrap();
    assert_eq!(again.layers.len(), p.layers.len());
    // The opening flash decays: exposure is 4 stops on beat 0 and 0 by beat 1.
    let e = Evaluator::new(&p);
    let fx = &p.layers[0].effects[1];
    assert_eq!(e.effect(fx, 0.0).num("stops"), 4.0);
    assert!(e.effect(fx, 0.25).num("stops") < 2.0); // three frames in, 0.75^3 of it remains
    assert!(e.effect(fx, 0.5).num("stops") < 1.0);
    assert_eq!(e.effect(fx, 1.0).num("stops"), 0.0);
    assert!(describe::describe(&p).contains("cube"));
}
