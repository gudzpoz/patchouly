use patchouly_core::stencils::Location;
use smallvec::SmallVec;

#[derive(Default, Debug)]
pub struct StencilArgs {
    pub inputs: SmallVec<[Location; 4]>,
    pub outputs: SmallVec<[Location; 4]>,
}
impl StencilArgs {
    pub fn parse(inputs: &str, outputs: &str) -> Option<Self> {
        Some(StencilArgs {
            inputs: parse_args(inputs)?,
            outputs: parse_args(outputs)?,
        })
    }
}

fn parse_args(args: &str) -> Option<SmallVec<[Location; 4]>> {
    if args.is_empty() {
        return Some(SmallVec::new());
    }

    let split = args.split("_");
    let mut args = SmallVec::new();
    for arg in split {
        if arg == "0" {
            args.push(Location::Stack(0));
        } else {
            let arg: u16 = arg.parse().ok()?;
            args.push(Location::Register(arg - 1));
        }
    }
    Some(args)
}

#[cfg(test)]
mod tests {
    use patchouly_core::stencils::io_to_index;

    use super::*;

    #[test]
    fn test_parse_args() {
        let assertions = [("1", "0", 10, 10), ("9", "0", 10, 90)];
        for (inputs, outputs, max_args, index) in assertions {
            let args = StencilArgs::parse(inputs, outputs).unwrap();
            assert_eq!(io_to_index(&args.inputs, &args.outputs, max_args), index);
        }
    }
}
