use crate::ast::{self, annotation, kw};
use crate::parser::{Parse, Parser, Result};

/// A parsed WebAssembly component module.
#[derive(Debug)]
pub struct Component<'a> {
    /// Where this `component` was defined
    pub span: ast::Span,
    /// An optional identifier this component is known by
    pub id: Option<ast::Id<'a>>,
    /// An optional `@name` annotation for this component
    pub name: Option<ast::NameAnnotation<'a>>,
    /// What kind of component this was parsed as.
    pub kind: ComponentKind<'a>,
}

/// The different kinds of ways to define a component.
#[derive(Debug)]
pub enum ComponentKind<'a> {
    /// A component defined in the textual s-expression format.
    Text(Vec<ComponentField<'a>>),
    /// A component that had its raw binary bytes defined via the `binary`
    /// directive.
    Binary(Vec<&'a [u8]>),
}

impl<'a> Component<'a> {
    /// Performs a name resolution pass on this [`Component`], resolving all
    /// symbolic names to indices.
    ///
    /// The WAT format contains a number of shorthands to make it easier to
    /// write, such as inline exports, inline imports, inline type definitions,
    /// etc. Additionally it allows using symbolic names such as `$foo` instead
    /// of using indices. This module will postprocess an AST to remove all of
    /// this syntactic sugar, preparing the AST for binary emission.  This is
    /// where expansion and name resolution happens.
    ///
    /// This function will mutate the AST of this [`Component`] and replace all
    /// [`super::Index`] arguments with `Index::Num`. This will also expand inline
    /// exports/imports listed on fields and handle various other shorthands of
    /// the text format.
    ///
    /// If successful the AST was modified to be ready for binary encoding. A
    /// [`ComponentNames`] structure is also returned so if you'd like to do your own
    /// name lookups on the result you can do so as well.
    ///
    /// # Errors
    ///
    /// If an error happens during resolution, such a name resolution error or
    /// items are found in the wrong order, then an error is returned.
    pub fn resolve(&mut self) -> std::result::Result<(), crate::Error> {
        // TODO: resolve for components

        Ok(())
    }

    /// Encodes this [`Component`] to its binary form.
    ///
    /// This function will take the textual representation in [`Component`] and
    /// perform all steps necessary to convert it to a binary WebAssembly
    /// component, suitable for writing to a `*.wasm` file. This function may
    /// internally modify the [`Component`], for example:
    ///
    /// * Name resolution is performed to ensure that `Index::Id` isn't present
    ///   anywhere in the AST.
    ///
    /// * Inline shorthands such as imports/exports/types are all expanded to be
    ///   dedicated fields of the component.
    ///
    /// * Component fields may be shuffled around to preserve index ordering from
    ///   expansions.
    ///
    /// After all of this expansion has happened the component will be converted to
    /// its binary form and returned as a `Vec<u8>`. This is then suitable to
    /// hand off to other wasm runtimes and such.
    ///
    /// # Errors
    ///
    /// This function can return an error for name resolution errors and other
    /// expansion-related errors.
    pub fn encode(&mut self) -> std::result::Result<Vec<u8>, crate::Error> {
        self.resolve()?;
        Ok(crate::binary::encode_component(self))
    }

    pub(super) fn validate(&self, parser: Parser<'_>) -> Result<()> {
        let mut starts = 0;
        if let ComponentKind::Text(fields) = &self.kind {
            for item in fields.iter() {
                if let ComponentField::Start(_) = item {
                    starts += 1;
                }
            }
        }
        if starts > 1 {
            return Err(parser.error("multiple start sections found"));
        }
        Ok(())
    }
}

impl<'a> Parse<'a> for Component<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parens(|parser| {
            let _r = parser.register_annotation("custom");

            let span = parser.parse::<kw::component>()?.0;
            let id = parser.parse()?;
            let name = parser.parse()?;

            let kind = if parser.peek::<kw::binary>() {
                parser.parse::<kw::binary>()?;
                let mut data = Vec::new();
                while !parser.is_empty() {
                    data.push(parser.parse()?);
                }
                ComponentKind::Binary(data)
            } else {
                ComponentKind::Text(ComponentField::parse_remaining(parser)?)
            };
            Ok(Component {
                span,
                id,
                name,
                kind,
            })
        })
    }
}

/// A listing of all possible fields that can make up a WebAssembly component.
#[allow(missing_docs)]
#[derive(Debug)]
pub enum ComponentField<'a> {
    Type(ast::ComponentType<'a>),
    Import(ast::ComponentImport<'a>),
    Func(ast::ComponentFunc<'a>),
    Export(ast::ComponentExport<'a>),
    Start(Start<'a>),
    Custom(ast::Custom<'a>),
    Instance(ast::Instance<'a>),
    Module(ast::NestedModule<'a>),
    Component(ast::Component<'a>),
    Alias(ast::Alias<'a>),
}

impl<'a> ComponentField<'a> {
    fn parse_remaining(parser: Parser<'a>) -> Result<Vec<ComponentField>> {
        let mut fields = Vec::new();
        while !parser.is_empty() {
            fields.push(parser.parse()?);
        }
        Ok(fields)
    }
}

impl<'a> Parse<'a> for ComponentField<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        if parser.peek2::<kw::r#type>() {
            return Ok(ComponentField::Type(parser.parse()?));
        }
        if parser.peek2::<kw::import>() {
            return Ok(ComponentField::Import(parser.parse()?));
        }
        if parser.peek2::<kw::func>() {
            return Ok(ComponentField::Func(parser.parse()?));
        }
        if parser.peek2::<kw::export>() {
            return Ok(ComponentField::Export(parser.parse()?));
        }
        if parser.peek2::<kw::start>() {
            return Ok(ComponentField::Start(parser.parse()?));
        }
        if parser.peek2::<annotation::custom>() {
            return Ok(ComponentField::Custom(parser.parse()?));
        }
        if parser.peek2::<kw::instance>() {
            return Ok(ComponentField::Instance(parser.parse()?));
        }
        if parser.peek2::<kw::module>() {
            return Ok(ComponentField::Module(parser.parens(|parser| parser.parse())?));
        }
        if parser.peek2::<kw::component>() {
            return Ok(ComponentField::Component(parser.parse()?));
        }
        if parser.peek2::<kw::alias>() {
            return Ok(ComponentField::Alias(parser.parse()?));
        }
        Err(parser.error("expected valid component field"))
    }
}

/// A function to call at instantiation time.
#[derive(Debug)]
pub struct Start<'a> {
    /// The function to call.
    func: ast::ItemRef<'a, kw::func>,
    /// The arguments to pass to the function.
    args: Vec<ast::ItemRef<'a, kw::value>>,
    /// Name of the result value.
    result: ast::Id<'a>,
}

impl<'a> Parse<'a> for Start<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parens(|parser| {
            parser.parse::<kw::start>()?;
            let func = parser.parse::<ast::IndexOrRef<_>>()?.0;
            let mut args = Vec::new();
            while !parser.peek2::<kw::result>() {
                args.push(parser.parse()?);
            }
            let result = parser.parens(|parser| {
                parser.parse::<kw::result>()?;
                parser.parens(|parser| {
                    parser.parse::<kw::value>()?;
                    parser.parse()
                })
            })?;
            Ok(Start { func, args, result })
        })
    }
}
