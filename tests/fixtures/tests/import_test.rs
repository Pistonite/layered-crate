#[test]
fn import_from_fixture_and_compile_and_run() {
    // should have these exported
    assert_eq!(fixtures::add(3, 4), 7);
    assert_eq!(fixtures::sub(2, 2), 0);
    assert_eq!(fixtures::sub_system_2::sub2(), 42);
    assert_eq!(fixtures::sub_system_1::sub1(), 37); // re-exported from foo
}
