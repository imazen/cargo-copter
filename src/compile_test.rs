use super::*;

fn write_package(dir: &Path, name: &str, version: &str, manifest: &str, source: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = {name:?}\nversion = {version:?}\nedition = '2024'\n{manifest}"),
    )
    .unwrap();
    fs::write(dir.join("src/lib.rs"), source).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for (dir, version) in [("stable", "1.0.0"), ("wip", "1.0.1")] {
        write_package(
            &temp.path().join(dir),
            "copter-feature-base",
            version,
            "[workspace]\n[features]\ndefault = ['unwanted']\nunwanted = []\nwide = []\nmember = []",
            "#[cfg(feature = \"unwanted\")] compile_error!(\"defaults must stay disabled\");\n#[cfg(all(feature = \"wide\", feature = \"member\"))] pub fn enabled() {}\n",
        );
    }
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("Cargo.toml"), "[workspace]\nmembers = ['member', 'second']\nresolver = '3'\n[workspace.dependencies]\ncopter-feature-base = { path = '../stable', version = '1.0.0', features = ['wide'], default-features = false }\n").unwrap();
    for member in ["member", "second"] {
        write_package(
            &workspace.join(member),
            member,
            "0.1.0",
            "[dependencies]\ncopter-feature-base = { workspace = true, features = ['member'] }",
            "pub fn check() { copter_feature_base::enabled(); }\n",
        );
    }
    temp
}

fn assert_original(dir: &Path, manifest: &[u8], lock: Option<&[u8]>) {
    assert_eq!(fs::read(dir.join("Cargo.toml")).unwrap(), manifest);
    assert_eq!(fs::read(dir.parent().unwrap().join("Cargo.lock")).ok().as_deref(), lock);
}

#[test]
fn inherited_features_survive_and_sibling_baseline_is_clean() {
    let fixture = fixture();
    let member = fixture.path().join("workspace/member");
    let wip = fixture.path().join("wip");
    let manifest = fs::read(member.join("Cargo.toml")).unwrap();
    // A stale legacy backup must never replace current local edits.
    fs::write(member.join("Cargo.toml.original.txt"), "stale backup").unwrap();
    let baseline = run_three_step_ict(TestConfig::new(&member, "copter-feature-base")).unwrap();
    assert!(baseline.is_success(), "{baseline:#?}");
    assert_eq!(baseline.actual_version.as_deref(), Some("1.0.0"));
    assert_original(&member, &manifest, None);

    // Exercise both pre-existing and absent workspace lockfiles.
    assert!(compile_crate(&member, CompileStep::Fetch, None).unwrap().success);
    let lock = fs::read(member.parent().unwrap().join("Cargo.lock")).unwrap();
    let offered =
        run_three_step_ict(TestConfig::new(&member, "copter-feature-base").with_override_path(&wip).with_version_info(
            Some("1.0.1".into()),
            true,
            Some("^1.0.0".into()),
        ))
        .unwrap();
    assert!(offered.is_success(), "{offered:#?}");
    assert_eq!(offered.actual_version.as_deref(), Some("1.0.1"));
    assert!(offered.check.unwrap().stdout.contains(&wip.display().to_string()));
    assert_original(&member, &manifest, Some(&lock));

    let second = fixture.path().join("workspace/second");
    let baseline = run_three_step_ict(TestConfig::new(&second, "copter-feature-base")).unwrap();
    assert!(baseline.is_success(), "{baseline:#?}");
    assert_eq!(baseline.actual_version.as_deref(), Some("1.0.0"));
    assert_original(&member, &manifest, Some(&lock));
    assert_eq!(fs::read_to_string(member.join("Cargo.toml.original.txt")).unwrap(), "stale backup");
}

#[test]
fn cleanup_covers_fetch_check_test_failures_and_setup_errors() {
    let fixture = fixture();
    let member = fixture.path().join("workspace/member");
    let wip = fixture.path().join("wip");
    let manifest = fs::read(member.join("Cargo.toml")).unwrap();
    let missing = fixture.path().join("missing");
    let fetch = run_three_step_ict(
        TestConfig::new(&member, "copter-feature-base")
            .with_override_path(&missing)
            .with_version_info(None, true, None),
    )
    .unwrap();
    assert!(!fetch.fetch.success);
    assert!(fetch.check.is_none());
    assert_original(&member, &manifest, None);

    fs::write(wip.join("src/lib.rs"), "pub fn removed() {}\n").unwrap();
    let check = run_three_step_ict(
        TestConfig::new(&member, "copter-feature-base").with_override_path(&wip).with_version_info(None, true, None),
    )
    .unwrap();
    assert!(check.fetch.success);
    assert!(!check.check.unwrap().success);
    assert!(check.test.is_none());
    assert_original(&member, &manifest, None);

    fs::write(wip.join("src/lib.rs"), "pub fn enabled() { panic!(\"intentional WIP regression\"); }\n").unwrap();
    fs::write(member.join("src/lib.rs"), "#[test] fn regression() { copter_feature_base::enabled(); }\n").unwrap();
    let test = run_three_step_ict(
        TestConfig::new(&member, "copter-feature-base").with_override_path(&wip).with_version_info(None, true, None),
    )
    .unwrap();
    assert!(test.fetch.success);
    assert!(test.check.unwrap().success);
    assert!(!test.test.unwrap().success);
    assert_original(&member, &manifest, None);

    // Error return after fetch (dependency absent), rather than a failed step.
    let error = run_three_step_ict(TestConfig::new(&member, "not-a-dependency"));
    assert!(error.unwrap_err().contains("not a dependency"));
    assert_original(&member, &manifest, None);
}

#[test]
fn override_covers_renames_tables_targets_and_additive_features() {
    let fixture = fixture();
    let member = fixture.path().join("workspace/member");
    let wip = fixture.path().join("wip");
    let root = fixture.path().join("workspace/Cargo.toml");
    let mut root_text = fs::read_to_string(&root).unwrap();
    root_text.push_str("alias = { package = 'copter-feature-base', path = '../stable', default-features = false, features = ['wide'] }\n");
    fs::write(&root, root_text).unwrap();
    write_package(
        &member,
        "member",
        "0.1.0",
        "[dependencies.alias]\nworkspace = true\nfeatures = ['member', 'wide']\noptional = true\n[target.'cfg(unix)'.build-dependencies]\ncopter-feature-base = { workspace = true, features = ['member'] }\n[dev-dependencies]\ncopter-feature-base = { path = '../../stable', default-features = false, features = ['wide', 'member'] }",
        "",
    );
    let files = TestFiles::capture(&member).unwrap();
    apply_dependency_override(&member, "copter-feature-base", &wip, DependencyOverrideMode::Force).unwrap();
    let doc: toml_edit::DocumentMut = fs::read_to_string(member.join("Cargo.toml")).unwrap().parse().unwrap();
    for dep in [
        &doc["dependencies"]["alias"],
        &doc["target"]["cfg(unix)"]["build-dependencies"]["copter-feature-base"],
        &doc["dev-dependencies"]["copter-feature-base"],
    ] {
        assert_eq!(dep["path"].as_str(), wip.to_str());
        assert_eq!(dep["default-features"].as_bool(), Some(false));
        let features: Vec<_> = dep["features"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(features, ["wide", "member"]);
        assert!(dep.get("workspace").is_none());
    }
    assert_eq!(doc["dependencies"]["alias"]["package"].as_str(), Some("copter-feature-base"));
    assert_eq!(doc["dependencies"]["alias"]["optional"].as_bool(), Some(true));
    drop(files);
}
