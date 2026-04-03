use patchouly_build::StencilSetup;

fn main() {
    StencilSetup::new("bf-stencils")
        .extract_and_emit()
        .expect("failed to extract stencils");
}
