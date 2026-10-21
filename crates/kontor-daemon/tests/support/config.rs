use std::path::{Path, PathBuf};

pub(crate) fn write_config_folder(root: &Path) -> PathBuf {
    fn copy(source: &Path, target: &Path) {
        std::fs::create_dir_all(target).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                copy(&path, &target.join(entry.file_name()));
            } else {
                std::fs::copy(&path, target.join(entry.file_name())).unwrap();
            }
        }
    }
    let folder = root.join("selected config with spaces");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/examples"),
        &folder,
    );
    folder
}

pub(crate) fn write_runtime_planes(folder: &Path, planes: &serde_json::Value) {
    let host = serde_json::json!({"schema_version":1,"runtime":{"planes":planes}});
    std::fs::write(
        folder.join("host.yml"),
        serde_yaml_ng::to_string(&host).unwrap(),
    )
    .unwrap();
}
