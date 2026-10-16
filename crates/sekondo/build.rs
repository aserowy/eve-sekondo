fn main() {
    // `sqlx::migrate!` and `#[sqlx::test]` embed the migrations at compile time.
    println!("cargo:rerun-if-changed=migrations");
}
