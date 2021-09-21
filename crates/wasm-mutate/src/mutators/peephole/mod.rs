use std::collections::HashMap;

use rand::{
    prelude::{IteratorRandom, SliceRandom, SmallRng},
    Rng,
};
use wasm_encoder::{CodeSection, Function, Module};
use wasmparser::{BinaryReaderError, CodeSectionReader, FunctionBody, Operator};

use crate::{
    mutators::peephole::swap_commutative::SwapCommutativeOperator, ModuleInfo, Result, WasmMutate,
};

use super::Mutator;

pub mod swap_commutative;

pub struct PeepholeMutator;

/// Meta mutator for peephole
impl Mutator for PeepholeMutator {
    fn mutate(
        &self,
        config: &crate::WasmMutate,
        rnd: &mut rand::prelude::SmallRng,
        info: &mut crate::ModuleInfo,
    ) -> Result<Module> {
        // Parse the module to get opcodes
        let mut codes = CodeSection::new();
        let code_section = info.get_code_section();
        let mut sectionreader = CodeSectionReader::new(code_section.data, 0)?;
        let function_count = sectionreader.get_count();

        let peep_optimizers: &Vec<Box<dyn CodeMutator>> = &vec![Box::new(SwapCommutativeOperator)];

        // Split where to start looking for mutable function
        let function_to_mutate = rnd.gen_range(0, function_count);
        let all_readers = (0..function_count)
            .map(|fidx| sectionreader.read().unwrap())
            .collect::<Vec<FunctionBody>>();

        // Since we can have several positions for the same mutator it is better to group them by mutator reference
        let mut applicable: HashMap<String, Vec<(usize, usize, &Box<dyn CodeMutator>)>> =
            HashMap::new();

        (function_to_mutate..function_count)
            .chain(0..function_to_mutate)
            .fold(&mut applicable, |prev, fidx| {
                let reader = all_readers[fidx as usize];
                let operatorreader = reader.get_operators_reader().unwrap();
                let operators = &operatorreader
                    .into_iter()
                    .collect::<wasmparser::Result<Vec<Operator>>>()
                    .unwrap();
                let operatorscount = operators.len();

                let opcode_to_mutate = rnd.gen_range(0, operatorscount);
                (opcode_to_mutate..operatorscount)
                    .chain(0..opcode_to_mutate)
                    .fold(prev, |innerprev, idx| {
                        for peephole in peep_optimizers {
                            if peephole.can_mutate(config, &operators, idx).unwrap() {
                                // We can have several mutators, lets group by mutator
                                // TODO, find better key ?
                                innerprev
                                    .entry(peephole.name())
                                    .or_insert(Vec::new())
                                    .push((fidx as usize, idx, peephole));
                            }
                        }
                        innerprev
                    })
            });

        // If no mutators, return specific error

        if applicable.len() == 0 {
            return Err(crate::Error::NotMatchingPeepholes);
        };

        let mutatoridx = applicable.keys().choose(rnd).unwrap();
        let positions = &applicable[mutatoridx];
        let (function_to_mutate, operatoridx, mutator) = positions.choose(rnd).unwrap();

        for fidx in 0..function_count as usize {
            let mut reader = all_readers[fidx];
            if fidx == *function_to_mutate {
                log::debug!("Mutating function idx {:?}", fidx);
                let function = mutator
                    .mutate(config, rnd, &mut reader, *operatoridx, &code_section.data)
                    .unwrap();
                println!("{:?}", function);
                codes.function(&function);
            } else {
                // Copy exactly the same function to section
                println!(
                    "{:?}",
                    &code_section.data[reader.range().start..reader.range().end]
                );
                codes.raw(&code_section.data[reader.range().start..reader.range().end]);
            }
        }

        let module = info.replace_section(info.code.unwrap(), &codes);
        Ok(module)
    }

    fn can_mutate<'a>(
        &self,
        config: &'a crate::WasmMutate,
        info: &crate::ModuleInfo,
    ) -> Result<bool> {
        Ok(info.has_code() && info.function_count > 0)
    }
}

use std::fmt::Debug;
impl Debug for Box<dyn CodeMutator> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Code mutator").finish()
    }
}
pub(crate) trait CodeMutator {
    fn mutate(
        &self,
        config: &WasmMutate,
        rnd: &mut SmallRng,
        funcreader: &mut FunctionBody,
        operator_index: usize,
        function_data: &[u8],
    ) -> Result<Function>;

    /// Returns if this mutator can be applied to the opcode at index i
    fn can_mutate<'a>(
        &self,
        config: &'a WasmMutate,
        operators: &Vec<Operator<'a>>,
        at: usize,
    ) -> Result<bool>;

    /// Provides the name of the mutator, mostly used for debugging purposes
    fn name(&self) -> String {
        return format!("{:?}", std::any::type_name::<Self>());
    }
}

// This macro is meant to be used for testing deep mutators
// It receives the original wat text variable, the expression returning the mutated function and the expected wat
// For an example, look at SwapCommutativeOperator
#[macro_export]
macro_rules! match_code_mutation {
    ($wat: ident, $mutation:expr, $expected:ident) => {{
        let original = &wat::parse_str($wat).unwrap();

        let mut parser = Parser::new(0);
        let config = WasmMutate::default();

        let mut offset = 0;

        let mut modu = Module::new();
        let mut codesection = CodeSection::new();

        loop {
            let (payload, chunksize) = match parser.parse(&original[offset..], true).unwrap() {
                Chunk::NeedMoreData(_) => {
                    panic!("This should not be reached");
                }
                Chunk::Parsed { consumed, payload } => (payload, consumed),
            };
            offset += chunksize;

            match payload {
                Payload::TypeSection(reader) => {
                    modu.section(&RawSection {
                        id: SectionId::Type.into(),
                        data: &original[reader.range().start..reader.range().end],
                    });
                }
                Payload::FunctionSection(reader) => {
                    modu.section(&RawSection {
                        id: SectionId::Function.into(),
                        data: &original[reader.range().start..reader.range().end],
                    });
                }
                Payload::CodeSectionEntry(reader) => {
                    let mutated = $mutation(&config, reader, original);
                    codesection.function(&mutated);
                }
                wasmparser::Payload::End => break,
                _ => {
                    // do nothing
                }
            }
        }
        modu.section(&codesection);
        let text = wasmprinter::print_bytes(modu.finish()).unwrap();

        // parse expected to use the same formatter
        let expected_bytes = &wat::parse_str($expected).unwrap();
        let expectedtext = wasmprinter::print_bytes(expected_bytes).unwrap();

        assert_eq!(text, expectedtext);
    }};
}

#[cfg(test)]
mod tests {
    use crate::{
        mutators::{peephole::PeepholeMutator, Mutator},
        WasmMutate,
    };
    use rand::{rngs::SmallRng, SeedableRng};

    #[test]
    fn test_peephole_mutator() {
        // From https://developer.mozilla.org/en-US/docs/WebAssembly/Text_format_to_wasm
        let wat = r#"
        (module
            (func (export "exported_func") (result i32) (local i32 i32)
                i32.const 42
                i32.const 42
                i32.add
                i32.const 56
                i32.add
            )
            (func (export "exported_func3") (result i32) (local i32 i32)
                i32.const 42
                i32.const 42
                i32.add
            )
        )
        "#;

        let wasmmutate = WasmMutate::default();
        let original = &wat::parse_str(wat).unwrap();

        let mutator = PeepholeMutator; // the string is empty

        let mut info = wasmmutate.get_module_info(original).unwrap();
        let can_mutate = mutator.can_mutate(&wasmmutate, &info).unwrap();

        assert_eq!(can_mutate, true);

        let mut rnd = SmallRng::seed_from_u64(2);
        let mutation = mutator.mutate(&wasmmutate, &mut rnd, &mut info);

        let mutation_bytes = mutation.unwrap().finish();

        let text = wasmprinter::print_bytes(mutation_bytes).unwrap();
        println!("{}", text);
    }
}
