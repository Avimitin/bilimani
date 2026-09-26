use chart_requester::output::{TextFile, resolved_output};
#[test]
fn unicode_replacement_is_complete_and_leaves_no_temporary_files() {
    let root = std::env::temp_dir().join(format!("chart-requester-test-{}", uuid::Uuid::new_v4()));
    let path = root.join("obs/queue.txt");
    let path = resolved_output(&path).unwrap();
    let mut file = TextFile::new(path.clone());
    file.write("冥\nAA -rebuild-\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "冥\nAA -rebuild-\n"
    );
    file.write("短\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "短\n");
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root.join("obs")).unwrap();
    std::fs::remove_dir(root).unwrap();
}
