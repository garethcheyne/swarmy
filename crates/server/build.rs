fn main() {
    // Re-embed the web UI whenever `pnpm build` produces a new web/dist.
    println!("cargo:rerun-if-changed=../../web/dist");
}
