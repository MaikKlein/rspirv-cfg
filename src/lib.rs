extern crate rspirv;
extern crate spirv_headers as spirv;
use rspirv::binary::Disassemble;
use rspirv::dr::{Block, Function, Instruction, Module, Operand};
use std::collections::{BTreeMap, HashSet};
use std::fs::{read, File};
use std::io::Write;
use std::path::Path;

macro_rules! extract {
    ($val:expr, $name:path) => {
        match $val {
            $name(inner) => inner,
            ref rest => panic!("Extract failed for {}, found {:?}", stringify!($name), rest),
        }
    };
}

fn disassemble_inststruction(module: &SpirvModule, inst: &Instruction) -> String {
    format!(
        "{rid}{opcode}{rtype}{space}{operands}",
        rid = {
            escape_html(
                &module
                    .name_or_id(inst.result_id)
                    .map(|name| format!("{} = ", name))
                    .unwrap_or_else(String::new),
            )
        },
        opcode = format!("Op{}", inst.class.opname),
        // extra space both before and after the reseult type
        rtype = escape_html(
            &module
                .name_or_id(inst.result_type)
                .map(|name| format!(" {}", name))
                .unwrap_or_else(String::new)
        ),
        //rtype = "",
        space = if !inst.operands.is_empty() { " " } else { "" },
        operands = {
            inst.operands
                .iter()
                .map(|operand| match *operand {
                    Operand::IdRef(id) => escape_html(&module.name_or_id(Some(id)).unwrap()),
                    _ => operand.disassemble(),
                })
                .collect::<Vec<String>>()
                .join(" ")
        }
    )
}
pub fn escape_html(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("\"", "&quot;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
}
pub struct SpirvModule {
    pub module: Module,
    pub names: BTreeMap<u32, String>,
}
impl SpirvModule {
    pub fn name_or_id(&self, id: Option<spirv::Word>) -> Option<String> {
        let id = id?;
        Some(
            self.names
                .get(&id)
                .map(|name| format!("{}(%{})", name, id))
                .unwrap_or_else(|| format!("%{}", id)),
        )
    }
    pub fn get_name_fn<'module>(&'module self, f: &Function) -> Option<&'module str> {
        let id = f.def.as_ref()?.result_id?;
        self.names.get(&id).map(String::as_str)
    }
    pub fn get_name_bb<'module>(&'module self, bb: &Block) -> Option<&'module str> {
        let label = bb.label.as_ref()?;
        let return_id = label.result_id?;
        self.names.get(&return_id).map(String::as_str)
    }
    pub fn load<P: AsRef<Path>>(p: &P) -> Self {
        fn inner(p: &Path) -> SpirvModule {
            use rspirv::binary::Parser;
            use rspirv::dr::Loader;
            let module = {
                let spv_file = read(p).expect("file");
                let mut loader = Loader::new();
                {
                    let p = Parser::new(&spv_file, &mut loader);
                    p.parse().expect("parse")
                };
                loader.module()
            };
            let names = module
                .debugs
                .iter()
                .filter_map(|inst| match inst.class.opcode {
                    spirv::Op::Name => {
                        let id = extract!(inst.operands[0], Operand::IdRef);
                        let name = extract!(&inst.operands[1], Operand::LiteralString);
                        Some((id, name.clone()))
                    }
                    _ => None,
                })
                .collect();
            SpirvModule { names, module }
        }
        inner(p.as_ref())
    }
}

pub struct PetSpirv<'spir> {
    pub module: &'spir SpirvModule,
    pub function: &'spir Function,
    pub block_map: BTreeMap<u32, &'spir Block>,
}

pub fn export_spirv_cfg(module: &SpirvModule) {
    let mut file = File::create("test.dot").expect("file");
    for f in &module.module.functions {
        let s = PetSpirv::new(module, f);
        s.add_fn_to_dot(&mut file);
    }
}
pub enum Terminator {
    Branch {
        target: spirv::Word,
    },
    BranchConditional {
        merge_block: Option<spirv::Word>,
        true_block: spirv::Word,
        false_block: spirv::Word,
    },
    Switch {
        merge_block: Option<spirv::Word>,
        values: Vec<spirv::Word>,
        targets: Vec<spirv::Word>,
    },
    /// Indicates that the control flow has stopped
    End,
}

impl Terminator {
    pub fn merge_block(&self) -> Option<spirv::Word> {
        match self {
            Terminator::Switch { merge_block, .. }
            | Terminator::BranchConditional { merge_block, .. } => *merge_block,
            _ => None,
        }
    }
    pub fn from_basic_block(bb: &Block) -> Terminator {
        let get_merge_block = || -> Option<spirv::Word> {
            let before_last = bb.instructions.get(bb.instructions.len() - 2)?;
            match before_last.class.opcode {
                spirv::Op::SelectionMerge => {
                    Some(extract!(before_last.operands[0], Operand::IdRef))
                }
                _ => None,
            }
        };
        let inst = if let Some(inst) = bb.instructions.last() {
            inst
        } else {
            return Terminator::End;
        };
        match inst.class.opcode {
            spirv::Op::Switch => {
                let default = extract!(inst.operands[1], Operand::IdRef);
                let merge_block = get_merge_block();
                let values: Vec<u32> = inst
                    .operands
                    .iter()
                    .skip(2)
                    .step_by(2)
                    .map(|operand| *extract!(operand, Operand::LiteralInt32))
                    .collect();
                let mut targets: Vec<spirv::Word> = inst
                    .operands
                    .iter()
                    .skip(3)
                    .step_by(2)
                    .map(|operand| *extract!(operand, Operand::IdRef))
                    .collect();
                targets.push(default);
                Terminator::Switch {
                    merge_block,
                    values,
                    targets,
                }
            }
            spirv::Op::BranchConditional => {
                let merge_block = get_merge_block();
                let true_block = extract!(inst.operands[1], Operand::IdRef);
                let false_block = extract!(inst.operands[2], Operand::IdRef);
                Terminator::BranchConditional {
                    merge_block,
                    true_block,
                    false_block,
                }
            }
            spirv::Op::Branch => {
                let target = extract!(inst.operands[0], Operand::IdRef);
                Terminator::Branch { target }
            }
            _ => Terminator::End,
        }
    }
    pub fn successors(&self) -> impl Iterator<Item = spirv::Word> {
        match self {
            Terminator::Switch { ref targets, .. } => targets.clone(),
            Terminator::Branch { target } => vec![*target],
            Terminator::BranchConditional {
                true_block,
                false_block,
                ..
            } => vec![*true_block, *false_block],
            _ => Vec::new(),
        }
        .into_iter()
    }
}
impl<'spir> PetSpirv<'spir> {
    pub fn get_block(&self, id: u32) -> &'spir Block {
        self.block_map
            .get(&id)
            .unwrap_or_else(|| panic!("Block {}", id))
    }
    pub fn add_fn_to_dot(&self, write: &mut impl Write) {
        let fn_name = self.module.get_name_fn(&self.function).unwrap_or("Unknown");
        writeln!(write, "digraph {{").unwrap();
        // HACK(eddyb) the `13.5` font size works around a graphviz bug where the
        // default size of `14` would compute metrics as `13.5` but render as `14`,
        // resulting in overflowing text for SVG, and generally differ from PNG.
        writeln!(write, "graph [fontname=\"monospace\", fontsize=13.5];").unwrap();
        writeln!(write, "node [fontname=\"monospace\", fontsize=13.5];").unwrap();
        writeln!(write, "edge [fontname=\"monospace\", fontsize=13.5];").unwrap();
        let fn_id = self.function.def.as_ref().unwrap().result_id.unwrap();
        let entry = self.function.blocks[0]
            .label
            .as_ref()
            .expect("label")
            .result_id
            .expect("result id");
        writeln!(
            write,
            "{id} [shape=\"box\", label={name:?}];",
            id = fn_id,
            name = fn_name
        )
        .unwrap();
        writeln!(write, "{} -> {}", fn_id, entry).unwrap();

        for (id, block) in &self.block_map {
            let name = self.module.name_or_id(Some(*id)).expect("name");
            writeln!(write, "  {id} [shape=none, label=<", id = id,).unwrap();
            writeln!(write, "\t<table>").unwrap();
            writeln!(
                write,
                "\t\t<tr><td align=\"center\" bgcolor=\"gray\" colspan=\"1\">{name}</td></tr>",
                name = name
            )
            .unwrap();
            writeln!(write, "\t\t<tr><td align=\"left\" balign=\"left\">").unwrap();
            for inst in &block.instructions {
                writeln!(
                    write,
                    "\t\t\t{}<br/>",
                    disassemble_inststruction(&self.module, inst)
                )
                .unwrap();
            }
            writeln!(write, "\t</td></tr></table>>];").unwrap();
        }

        self.traverse(|node, _| {
            let terminator = Terminator::from_basic_block(self.get_block(node));
            if let Some(merge_block) = terminator.merge_block() {
                writeln!(write, "\t{} -> {}[style=\"dashed\"]", node, merge_block).unwrap();
            }
            for bb in terminator.successors() {
                writeln!(write, "  {node} -> {target}", node = node, target = bb).unwrap();
            }
        });
        writeln!(write, "}}").unwrap();
    }

    pub fn get_label(&self, id: u32) -> String {
        self.module
            .names
            .get(&id)
            .cloned()
            .unwrap_or(format!("{}", id))
    }

    pub fn traverse(&self, mut f: impl FnMut(u32, &Terminator)) {
        let mut map = HashSet::new();
        if let Some(start_block) = self.function.blocks.first() {
            let label = start_block.label.as_ref().unwrap();
            let id = label.result_id.unwrap();
            self.traverse_from(&mut map, id, &mut f);
        }
    }

    fn traverse_from(
        &self,
        visited: &mut HashSet<u32>,
        root_id: u32,
        f: &mut impl FnMut(u32, &Terminator),
    ) {
        visited.insert(root_id);
        let root = self.get_block(root_id);
        let terminator = Terminator::from_basic_block(root);
        f(root_id, &terminator);
        for bb in terminator.successors() {
            if !visited.contains(&bb) {
                self.traverse_from(visited, bb, f);
            }
        }
    }

    pub fn new(module: &'spir SpirvModule, function: &'spir Function) -> Self {
        let block_map = function
            .blocks
            .iter()
            .filter_map(|bb| {
                let label = bb.label.as_ref()?;
                label.result_id.map(|id| (id, bb))
            })
            .collect();
        PetSpirv {
            module,
            function,
            block_map,
        }
    }
}
