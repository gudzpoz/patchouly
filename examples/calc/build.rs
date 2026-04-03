use patchouly_build::StencilSetup;

fn main() {
    StencilSetup::new("calc-stencils")
        .extract_and_emit()
        .expect("failed to extract lisp-jit stencils");
}
