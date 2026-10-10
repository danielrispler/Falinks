//! Linux containment helper (ADR 2): runs one contained command and reports its outcome.
fn main() {
    std::process::exit(falinks::helper_main());
}
