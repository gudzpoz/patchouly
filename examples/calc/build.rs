use patchouly_build::StencilSetup;

fn main() {
    StencilSetup::new("calc-stencils")
        .extract_and_emit()
        .expect("failed to extract calc test stencils");
}
