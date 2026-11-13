use edits_core::{check, describe, Blend, Project};

fn with_blend(name: &str) -> anyhow::Result<Project> {
    Project::from_json(&format!(
        r#"{{"format":"edits.project/1","settings":{{"width":64,"height":64,"fps":10,"duration":1}},
            "layers":[{{"id":"s","kind":"solid","color":[1,0,0,1],"blend":"{name}"}}]}}"#
    ))
}

#[test]
fn check_accepts_every_blend_mode_by_its_name() {
    let names: Vec<&str> = Blend::ALL.iter().map(|b| b.name()).collect();
    for want in [
        "normal", "add", "screen", "multiply", "overlay", "soft_light", "hard_light", "color_dodge", "color_burn", "linear_dodge", "linear_burn",
        "vivid_light", "linear_light", "pin_light", "hard_mix", "difference", "exclusion", "subtract", "divide", "lighten", "darken", "hue",
        "saturation", "color", "luminosity", "lighter_color", "darker_color",
    ] {
        assert!(names.contains(&want), "{want} is not a blend mode");
    }
    for mode in Blend::ALL {
        let p = with_blend(mode.name()).unwrap_or_else(|e| panic!("{}: {e}", mode.name()));
        assert_eq!(p.layers[0].blend, mode);
        assert!(!check::has_errors(&check::check(&p, None)), "{}", mode.name());
        // The name in files is the serialised name, and `describe` prints it.
        assert!(p.to_json().contains(&format!(r#""blend": "{}""#, mode.name())) || mode == Blend::Normal, "{}", mode.name());
        if mode != Blend::Normal {
            assert!(describe::describe(&p).contains(&format!("blend={}", mode.name())), "{}", mode.name());
        }
    }
}

#[test]
fn unknown_blend_modes_are_rejected_with_the_list() {
    for bad in ["softlight", "Soft Light", "burn", ""] {
        let err = with_blend(bad).err().unwrap_or_else(|| panic!("\"{bad}\" was accepted")).to_string();
        assert!(err.contains("unknown variant") && err.contains("soft_light"), "{bad}: {err}");
    }
}
