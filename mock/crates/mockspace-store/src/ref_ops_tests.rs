use super::staging_dir;

// Mutation: name the directory by the process id, as it was; what a commit hands git then has as
// many bytes more as the id has digits, and a cost counted in bytes moves with the pid.
#[test]
fn a_staging_directory_is_named_by_its_place_alone_and_is_its_own() {
    let tmp = tempfile::tempdir().unwrap();
    let a = staging_dir(tmp.path()).unwrap();
    let b = staging_dir(tmp.path()).unwrap();
    assert_eq!(a, tmp.path().join("mockspace-store-0.blobs"));
    assert_eq!(b, tmp.path().join("mockspace-store-1.blobs"), "one made and still there is passed over");
    assert!(a.is_dir() && b.is_dir());
    std::fs::remove_dir(&a).unwrap();
    assert_eq!(staging_dir(tmp.path()).unwrap(), a, "a name nobody holds is taken again");
}
