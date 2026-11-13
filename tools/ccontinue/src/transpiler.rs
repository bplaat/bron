/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::exit;
use std::{fs, iter};

use indexmap::IndexMap;
use regex::{Captures, Regex, regex};

use super::types::{Argument, Class, Field, Interface, Method};
use super::utils::{find_matching_close, parse_arguments, to_snake_case};

// MARK: Transpiler
/// Translates cContinue source code into C11 source code.
pub struct Transpiler {
    include_paths: Vec<String>,
    embedded_includes: HashMap<String, String>,
    classes: IndexMap<String, Class>,
    interfaces: IndexMap<String, Interface>,
    next_interface_id: usize,
    processed_includes: HashSet<String>,
}

impl Transpiler {
    /// Creates a transpiler with filesystem include search paths.
    pub fn new(include_paths: Vec<String>) -> Self {
        Transpiler {
            include_paths,
            embedded_includes: HashMap::new(),
            classes: IndexMap::new(),
            interfaces: IndexMap::new(),
            next_interface_id: 1,
            processed_includes: HashSet::new(),
        }
    }

    /// Replaces the in-memory include files available to the transpiler.
    pub fn set_embedded_includes(&mut self, map: HashMap<String, String>) {
        self.embedded_includes = map;
    }

    /// Clears state collected while transpiling the previous source file.
    pub fn reset(&mut self) {
        self.next_interface_id = 1;
        self.interfaces.clear();
        self.classes.clear();
        self.processed_includes.clear();
    }

    // MARK: Helpers
    fn find_class_for_method<'a>(&'a self, class_: &'a Class, method_name: &str) -> &'a Class {
        if class_.methods[method_name].class_ == class_.name {
            return class_;
        }
        if class_.parent_name.is_none() {
            eprintln!("[ERROR] No class implements method: {method_name}");
            exit(1);
        }
        self.find_class_for_method(
            &self.classes[class_.parent_name.as_ref().expect("parent_name is Some")],
            method_name,
        )
    }

    fn concrete_subclasses(&self, target_name: &str) -> Vec<String> {
        let mut result = Vec::new();
        for cls in self.classes.values() {
            if cls.is_abstract {
                continue;
            }
            let mut cur_name: Option<&str> = Some(&cls.name);
            while let Some(cn) = cur_name {
                if cn == target_name {
                    result.push(cls.name.clone());
                    break;
                }
                cur_name = self.classes.get(cn).and_then(|c| c.parent_name.as_deref());
            }
        }
        result
    }

    fn static_method_signature(&self, class_: &Class, method: &Method) -> String {
        let mut sig = format!(
            "{} {}_{}(",
            method.return_type, class_.snake_name, method.name
        );
        if !method.arguments.is_empty() {
            sig.push_str(
                &method
                    .arguments
                    .iter()
                    .map(|a| format!("{} {}", a.type_, a.name))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        } else {
            sig.push_str("void");
        }
        sig.push(')');
        sig
    }

    fn codegen_static_method_declaration(&self, class_: &Class, method: &Method) -> String {
        self.static_method_signature(class_, method) + ";\n"
    }

    fn codegen_static_method_definition(&self, class_: &Class, method: &Method) -> String {
        let mut code = self.static_method_signature(class_, method) + " {\n";
        if method.name == "new" {
            code += &format!(
                "    {}* this = malloc(sizeof({}));\n",
                class_.name, class_.name
            );
            code += &format!("    this->vtbl = &_{}Vtbl;\n", class_.name);
            code += &format!("    {}_init(", class_.snake_name);
            let args: Vec<String> = iter::once("this".to_owned())
                .chain(method.arguments.iter().map(|a| a.name.clone()))
                .collect();
            code += &args.join(", ");
            code += ");\n";
            code += "    return this;\n";
        }
        code += "}\n\n";
        code
    }

    // MARK: Convert includes
    fn convert_include(&mut self, current_path: &str, include_name: &str) -> String {
        let base_path = format!("{include_name}.hh");
        if !self.processed_includes.insert(base_path.clone()) {
            return String::new();
        }

        // Determine is_header by comparing stems: a .hh file is a "companion header"
        // (not an independent header) only when its stem matches the current file's stem.
        let include_stem = include_name
            .rsplit('/')
            .next()
            .expect("include_name has at least one component");
        let current_stem = Path::new(current_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let is_header = include_stem != current_stem;

        for include_path in &self.include_paths {
            let complete_path = format!("{include_path}/{base_path}");
            if Path::new(&complete_path).exists() {
                let text = fs::read_to_string(&complete_path).unwrap_or_else(|_| {
                    eprintln!("[ERROR] Can't read include: {complete_path}");
                    exit(1);
                });
                return self.transpile(&complete_path, is_header, &text);
            }
        }
        // Fallback: look up in embedded includes map
        if let Some(text) = self.embedded_includes.get(&base_path).cloned() {
            let virtual_path = format!("<embedded>/{base_path}");
            return self.transpile(&virtual_path, is_header, &text);
        }
        eprintln!("[ERROR] Can't find include: {base_path}");
        exit(1);
    }

    // MARK: Convert interfaces
    fn convert_interface(
        &mut self,
        iface_name: &str,
        supers_raw: Option<&str>,
        contents: &str,
    ) -> String {
        let re_iface_method = regex!(
            r"([_A-Za-z][_A-Za-z0-9 ]*[\**|\s+])\s*([_A-Za-z][_A-Za-z0-9]*)\(([^\)]*)\)\s*(=\s*0\s*)?;"
        );
        let snake_name = to_snake_case(iface_name);
        let id = self.next_interface_id;
        self.next_interface_id += 1;
        let mut iface = Interface::new(iface_name, id);

        if let Some(supers) = supers_raw {
            for name in supers.trim().trim_start_matches(':').split(',') {
                let name = name.trim();
                if name.is_empty() {
                    continue;
                }
                if !self.interfaces.contains_key(name) {
                    eprintln!("[ERROR] Can't find parent interface {name} for {iface_name}");
                    exit(1);
                }
                iface.parent_names.push(name.to_owned());
                let parent_methods: Vec<Method> =
                    self.interfaces[name].methods.values().cloned().collect();
                for method in parent_methods {
                    iface.methods.insert(method.name.clone(), method);
                }
            }
        }

        for caps in re_iface_method.captures_iter(contents) {
            let return_type = caps[1].replace("virtual ", "").trim().to_owned();
            let name = caps[2].to_owned();
            let arguments = parse_arguments(&caps[3]);
            iface.methods.insert(
                name.clone(),
                Method {
                    name: name.clone(),
                    return_type,
                    is_return_self: false,
                    is_virtual: true,
                    is_static: false,
                    arguments,
                    class_: iface_name.to_owned(),
                    origin_class: iface_name.to_owned(),
                },
            );
        }

        let mut c = format!("// interface {iface_name}\n");
        c += &format!("#define _{}_ID {}\n\n", iface_name, iface.id);

        c += &format!("typedef struct {iface_name}Vtbl {{\n");
        let mut current_origin = String::new();
        for method in iface.methods.values() {
            if method.origin_class != current_origin {
                c += &format!("    // {}\n", method.origin_class);
                current_origin = method.origin_class.clone();
            }
            c += &format!("    {} (*{})(void* this", method.return_type, method.name);
            for argument in &method.arguments {
                c += &format!(", {} {}", argument.type_, argument.name);
            }
            c += ");\n";
        }
        c += &format!("}} {iface_name}Vtbl;\n\n");

        c += &format!("typedef struct {iface_name} {{\n");
        c += "    void* obj;\n";
        c += &format!("    const {iface_name}Vtbl* vtbl;\n");
        c += &format!("}} {iface_name};\n\n");

        for method in iface.methods.values() {
            c += &format!("#define {}_{}(iface", snake_name, method.name);
            for argument in &method.arguments {
                c += &format!(", {}", argument.name);
            }
            c += &format!(") ((iface).vtbl->{}((iface).obj", method.name);
            for argument in &method.arguments {
                c += &format!(", ({})", argument.name);
            }
            c += "))\n";
        }
        c += "\n";

        self.interfaces.insert(iface_name.to_owned(), iface);
        c
    }

    // MARK: Convert classes: indexing
    fn index_class(
        &mut self,
        class_name: &str,
        supers_raw: Option<&str>,
        contents: &str,
    ) -> (String, Option<String>) {
        let re_field = regex!(r"(.+[^=][\s|\*])([_A-Za-z][_A-Za-z0-9]*)\s*(=\s*[^;]+)?;");
        let re_field_attr = regex!(r"@([_A-Za-z][_A-Za-z0-9]*)(\([^\)]*\))?");
        let re_method_decl = regex!(
            r"([_A-Za-z][_A-Za-z0-9 ]*[\**|\s+])\s*([_A-Za-z][_A-Za-z0-9]*)\(([^\)]*)\)\s*(=\s*0)?;"
        );
        let re_self_return = regex!(r"Self\s*\*");
        let mut parent_name: Option<String> = if class_name == "Object" {
            None
        } else {
            Some("Object".to_owned())
        };
        let mut explicit_interfaces: Vec<String> = Vec::new();

        if let Some(supers) = supers_raw {
            for name in supers.trim().trim_start_matches(':').split(',') {
                let name = name.trim();
                if name.is_empty() {
                    continue;
                }
                if self.classes.contains_key(name) {
                    parent_name = Some(name.to_owned());
                } else if self.interfaces.contains_key(name) {
                    if !explicit_interfaces.contains(&name.to_owned()) {
                        explicit_interfaces.push(name.to_owned());
                    }
                } else {
                    eprintln!("[ERROR] Unknown class or interface '{name}' in class {class_name}");
                    exit(1);
                }
            }
        }

        let parent_class_name: Option<String> = parent_name.clone();
        if let Some(ref pname) = parent_class_name
            && !self.classes.contains_key(pname.as_str())
        {
            eprintln!("[ERROR] Can't find parent class {pname} for {class_name}");
            exit(1);
        }

        let mut class_ = Class::new(class_name, parent_name.clone());
        if let Some(ref pname) = parent_class_name {
            let parent = &self.classes[pname.as_str()];
            class_.fields = parent.fields.clone();
            class_.methods = parent.methods.clone();
            class_.interface_names = parent.interface_names.clone();
        }

        // Add interfaces (recursive)
        fn add_interface(
            iface_name: &str,
            class_: &mut Class,
            interfaces: &IndexMap<String, Interface>,
        ) {
            if class_.interface_names.contains(&iface_name.to_owned()) {
                return;
            }
            class_.interface_names.push(iface_name.to_owned());
            let parent_names: Vec<String> = interfaces[iface_name].parent_names.clone();
            for pname in &parent_names {
                add_interface(pname, class_, interfaces);
            }
        }
        for iface_name in &explicit_interfaces {
            add_interface(iface_name, &mut class_, &self.interfaces);
        }

        // Auto-implement interfaces: if a class satisfies all parents of a zero-own-method
        // interface, automatically implement it (like Rust auto traits). Repeat until stable.
        loop {
            let mut changed = false;
            let all_iface_names: Vec<String> = self.interfaces.keys().cloned().collect();
            for iface_name in &all_iface_names {
                if class_.interface_names.contains(iface_name) {
                    continue;
                }
                let iface = &self.interfaces[iface_name.as_str()];
                if iface.parent_names.is_empty() {
                    continue;
                }
                if iface
                    .methods
                    .values()
                    .any(|m| &m.origin_class == iface_name)
                {
                    continue;
                }
                if iface
                    .parent_names
                    .iter()
                    .all(|p| class_.interface_names.contains(p))
                {
                    class_.interface_names.push(iface_name.clone());
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Index fields
        for caps in re_field.captures_iter(contents) {
            let attributes_and_type_str = &caps[1];
            let name = caps[2].to_owned();
            let default_str = caps.get(3).map(|m| m.as_str()).unwrap_or("");

            let mut attributes: IndexMap<String, Vec<String>> = IndexMap::new();
            for attr_m in re_field_attr.captures_iter(attributes_and_type_str) {
                let attr_name = attr_m[1].to_owned();
                let attr_args = attr_m.get(2).map(|m| m.as_str());
                let args: Vec<String> = if let Some(args_str) = attr_args {
                    args_str[1..args_str.len() - 1]
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .collect()
                } else {
                    Vec::new()
                };
                attributes.insert(attr_name, args);
            }
            let field_type = re_field_attr
                .replace_all(attributes_and_type_str, "")
                .trim()
                .to_owned();

            if class_.fields.contains_key(&name) {
                eprintln!("[ERROR] Can't inherit field: {name}");
                exit(1);
            }

            class_.fields.insert(
                name.clone(),
                Field {
                    name: name.clone(),
                    type_: field_type,
                    default: if default_str.is_empty() {
                        None
                    } else {
                        Some(default_str[1..].trim().to_owned())
                    },
                    attributes,
                    class_: class_name.to_owned(),
                },
            );
        }

        // Index methods
        for caps in re_method_decl.captures_iter(contents) {
            let mut return_type = caps[1].to_owned();
            let name = caps[2].to_owned();
            let arguments = parse_arguments(&caps[3]);
            let is_zero = caps.get(4).map(|m| m.as_str()).unwrap_or("");

            let mut is_static = false;
            if return_type.contains("static ") {
                return_type = return_type.replace("static ", "");
                is_static = true;
            }

            let mut is_virtual = false;
            if return_type.contains("virtual ") {
                return_type = return_type.replace("virtual ", "");
                is_virtual = true;
            }

            let mut is_return_self = false;
            if re_self_return.is_match(&return_type) {
                is_return_self = true;
                return_type = return_type.replace("Self", class_name);
            }

            if !is_zero.is_empty() {
                if is_virtual {
                    class_.is_abstract = true;
                } else {
                    eprintln!("[ERROR] Only virtual methods can be set to zero: {name}");
                    exit(1);
                }
            }

            if let Some(existing) = class_.methods.get_mut(&name) {
                existing.return_type = return_type;
                existing.arguments = arguments;
                existing.class_ = class_name.to_owned();
                existing.is_static = is_static;
            } else {
                class_.methods.insert(
                    name.clone(),
                    Method {
                        name: name.clone(),
                        return_type,
                        is_return_self,
                        is_virtual,
                        is_static,
                        arguments,
                        class_: class_name.to_owned(),
                        origin_class: class_name.to_owned(),
                    },
                );
            }
        }

        self.classes.insert(class_name.to_owned(), class_);
        (class_name.to_owned(), parent_class_name)
    }

    // MARK: Convert classes: codegen
    fn codegen_missing_methods(
        &mut self,
        class_name: &str,
        parent_class_name: &Option<String>,
        is_header: bool,
    ) -> String {
        let mut g = String::new();

        if parent_class_name.is_some() {
            let pname = parent_class_name
                .as_ref()
                .expect("parent class name is set")
                .clone();

            // Auto init
            let field_needs_init = self.classes[class_name]
                .fields
                .values()
                .any(|f| f.attributes.contains_key("init") || f.default.is_some());
            let init_class = self.classes[class_name].methods["init"].class_.clone();
            if init_class != class_name && field_needs_init {
                // Build init arguments
                let init_args: Vec<Argument> = self.classes[class_name]
                    .fields
                    .values()
                    .filter(|f| f.attributes.contains_key("init"))
                    .map(|f| Argument {
                        name: f.name.clone(),
                        type_: f.type_.clone(),
                    })
                    .collect();
                let parent_init_args: Vec<String> = self.classes[&pname].methods["init"]
                    .arguments
                    .iter()
                    .map(|a| a.name.clone())
                    .collect();

                self.classes
                    .get_mut(class_name)
                    .expect("class exists")
                    .methods
                    .get_mut("init")
                    .expect("init method exists")
                    .class_ = class_name.to_owned();
                self.classes
                    .get_mut(class_name)
                    .expect("class exists")
                    .methods
                    .get_mut("init")
                    .expect("init method exists")
                    .arguments = init_args.clone();

                let snake_name = self.classes[class_name].snake_name.clone();
                let parent_snake = to_snake_case(&pname);

                g += &format!("void _{snake_name}_init(");
                let sig_args: Vec<String> = iter::once(format!("{class_name}* this"))
                    .chain(init_args.iter().map(|a| format!("{} {}", a.type_, a.name)))
                    .collect();
                g += &sig_args.join(", ");
                g += ") {\n";
                g += &format!(
                    "    {}_init({});\n",
                    parent_snake,
                    iter::once("this".to_owned())
                        .chain(parent_init_args)
                        .collect::<Vec<_>>()
                        .join(", ")
                );

                // Field assignments for new fields
                let fields: Vec<Field> = self.classes[class_name]
                    .fields
                    .values()
                    .filter(|f| f.class_ == class_name)
                    .cloned()
                    .collect();
                for field in &fields {
                    if let Some(ref default) = field.default {
                        g += &format!("    this->{} = {};\n", field.name, default);
                    }
                    if field.attributes.contains_key("init") {
                        let init_attrs = &field.attributes["init"];
                        if !init_attrs.is_empty() {
                            g += &format!(
                                "    this->{} = {}({});\n",
                                field.name, init_attrs[0], field.name
                            );
                        } else {
                            g += &format!("    this->{} = {};\n", field.name, field.name);
                        }
                    }
                }
                g += "}\n\n";
            }

            // Auto deinit
            let field_needs_deinit = self.classes[class_name]
                .fields
                .values()
                .any(|f| f.class_ == class_name && f.attributes.contains_key("deinit"));
            if field_needs_deinit {
                self.classes
                    .get_mut(class_name)
                    .expect("class exists")
                    .methods
                    .get_mut("deinit")
                    .expect("deinit method exists")
                    .class_ = class_name.to_owned();

                let snake_name = self.classes[class_name].snake_name.clone();
                g += &format!("void _{snake_name}_deinit({class_name}* this) {{\n");

                let fields: Vec<Field> = self.classes[class_name]
                    .fields
                    .values()
                    .filter(|f| f.class_ == class_name && f.attributes.contains_key("deinit"))
                    .cloned()
                    .collect();
                for field in &fields {
                    let deinit_attrs = &field.attributes["deinit"];
                    if !deinit_attrs.is_empty() {
                        g += &format!("    {}(this->{});\n", deinit_attrs[0], field.name);
                    } else {
                        // Try to find matching class for free
                        let field_type = field.type_.clone();
                        let found_class = self
                            .classes
                            .values()
                            .find(|c| field_type.starts_with(&c.name))
                            .map(|c| c.snake_name.clone());
                        if let Some(sc) = found_class {
                            g += &format!("    {}_free(this->{});\n", sc, field.name);
                        } else {
                            g += &format!("    free(this->{});\n", field.name);
                        }
                    }
                }
                let class_with_deinit = self
                    .find_class_for_method(&self.classes[&pname], "deinit")
                    .name
                    .clone();
                let class_with_deinit_snake = to_snake_case(&class_with_deinit);
                g += &format!(
                    "    _{}_{}_deinit(({}*)this);\n",
                    class_with_deinit_snake, "deinit", class_with_deinit
                );
                // Simplify: just call _{snake_deinit_class}_deinit
                // Actually fix: need exact call
                g = g
                    .trim_end_matches(&format!(
                        "    _{}_{}_deinit(({}*)this);\n",
                        class_with_deinit_snake, "deinit", class_with_deinit
                    ))
                    .to_owned();
                g += &format!(
                    "    _{class_with_deinit_snake}_deinit(({class_with_deinit}*)this);\n"
                );
                g += "}\n\n";
            }

            // Auto getters
            let getter_fields: Vec<Field> = self.classes[class_name]
                .fields
                .values()
                .filter(|f| {
                    f.class_ == class_name
                        && (f.attributes.contains_key("get") || f.attributes.contains_key("prop"))
                })
                .cloned()
                .collect();
            for field in getter_fields {
                let method_name = format!("get_{}", field.name);
                let snake_name = self.classes[class_name].snake_name.clone();
                self.classes
                    .get_mut(class_name)
                    .expect("class exists")
                    .methods
                    .insert(
                        method_name.clone(),
                        Method {
                            name: method_name.clone(),
                            return_type: field.type_.clone(),
                            is_return_self: false,
                            is_virtual: false,
                            is_static: false,
                            arguments: Vec::new(),
                            class_: class_name.to_owned(),
                            origin_class: class_name.to_owned(),
                        },
                    );
                g += &format!(
                    "{} _{}_get_{}({}* this) {{\n",
                    field.type_, snake_name, field.name, class_name
                );
                g += &format!("    return this->{};\n", field.name);
                g += "}\n\n";
            }

            // Auto setters
            let setter_fields: Vec<Field> = self.classes[class_name]
                .fields
                .values()
                .filter(|f| {
                    f.class_ == class_name
                        && (f.attributes.contains_key("set") || f.attributes.contains_key("prop"))
                })
                .cloned()
                .collect();
            for field in setter_fields {
                let method_name = format!("set_{}", field.name);
                let snake_name = self.classes[class_name].snake_name.clone();
                self.classes
                    .get_mut(class_name)
                    .expect("class exists")
                    .methods
                    .insert(
                        method_name.clone(),
                        Method {
                            name: method_name.clone(),
                            return_type: "void".to_owned(),
                            is_return_self: false,
                            is_virtual: false,
                            is_static: false,
                            arguments: vec![Argument {
                                name: field.name.clone(),
                                type_: field.type_.clone(),
                            }],
                            class_: class_name.to_owned(),
                            origin_class: class_name.to_owned(),
                        },
                    );
                g += &format!(
                    "void _{}_set_{}({}* this, {} {}) {{\n",
                    snake_name, field.name, class_name, field.type_, field.name
                );
                g += &format!("    this->{} = {};\n", field.name, field.name);
                g += "}\n\n";
            }
        }

        // New method (all non-abstract classes)
        if !self.classes[class_name].is_abstract {
            let init_args: Vec<Argument> =
                self.classes[class_name].methods["init"].arguments.clone();
            let new_method = Method {
                name: "new".to_owned(),
                return_type: format!("{class_name}*"),
                is_return_self: false,
                is_virtual: false,
                is_static: true,
                arguments: init_args,
                class_: class_name.to_owned(),
                origin_class: class_name.to_owned(),
            };
            self.classes
                .get_mut(class_name)
                .expect("class exists")
                .methods
                .insert("new".to_owned(), new_method.clone());
            if !is_header {
                let class_ = &self.classes[class_name];
                g += &self.codegen_static_method_definition(class_, &new_method);
            }
        }

        g
    }

    fn codegen_class_struct(&self, class_name: &str) -> String {
        let class_ = &self.classes[class_name];
        let mut c = format!("typedef struct {0} {0};\n\n", class_.name);
        c += &format!("typedef struct {}Vtbl {{\n", class_.name);
        c += "    const _InterfaceSlot* interfaces;\n";
        let mut current_class_name = String::new();
        for method in class_.methods.values() {
            if method.is_virtual {
                if method.origin_class != current_class_name {
                    c += &format!("    // {}\n", method.origin_class);
                    current_class_name = method.origin_class.clone();
                }
                c += &format!("    {} (*{})(", method.return_type, method.name);
                c += &iter::once("void* this".to_owned())
                    .chain(
                        method
                            .arguments
                            .iter()
                            .map(|a| format!("{} {}", a.type_, a.name)),
                    )
                    .collect::<Vec<_>>()
                    .join(", ");
                c += ");\n";
            }
        }
        c += &format!("}} {}Vtbl;\n\n", class_.name);
        if !class_.is_abstract {
            c += &format!("extern {}Vtbl _{}Vtbl;\n\n", class_.name, class_.name);
        }
        c += &format!("struct {} {{\n", class_.name);
        c += &format!("    {}Vtbl* vtbl;\n", class_.name);
        let mut current_class_name = String::new();
        for field in class_.fields.values() {
            if field.class_ != current_class_name {
                c += &format!("    // {}\n", field.class_);
                current_class_name = field.class_.clone();
            }
            c += &format!("    {} {};\n", field.type_, field.name);
        }
        c += "};\n\n";
        c
    }

    fn codegen_class_forward_decls(&self, class_name: &str) -> String {
        let class_ = &self.classes[class_name];
        let mut c = String::new();
        if !class_.is_abstract
            && let Some(new_m) = class_.methods.get("new")
        {
            c += &self.codegen_static_method_declaration(class_, new_m);
        }
        for method in class_.methods.values() {
            if method.class_ == class_.name && method.name != "new" {
                if method.is_static {
                    c += &self.codegen_static_method_declaration(class_, method);
                } else {
                    c += &format!(
                        "{} _{}_{}(",
                        method.return_type, class_.snake_name, method.name
                    );
                    c += &iter::once(format!("{}* this", class_.name))
                        .chain(
                            method
                                .arguments
                                .iter()
                                .map(|a| format!("{} {}", a.type_, a.name)),
                        )
                        .collect::<Vec<_>>()
                        .join(", ");
                    c += ");\n";
                }
            }
        }
        c += "\n";
        c
    }

    fn codegen_class_vtbl_instance(&self, class_name: &str) -> String {
        let class_ = &self.classes[class_name];
        let mut c = String::new();

        if !class_.interface_names.is_empty() {
            for iface_name in &class_.interface_names {
                let iface = &self.interfaces[iface_name];
                for method in iface.methods.values() {
                    let adapter_name = format!(
                        "_{}_{}_{}_vcall",
                        class_.snake_name, iface.snake_name, method.name
                    );
                    c += &format!("static {} {adapter_name}(void* this", method.return_type);
                    for argument in &method.arguments {
                        c += &format!(", {} {}", argument.type_, argument.name);
                    }
                    c += ") {\n    ";
                    if method.return_type.trim() != "void" {
                        c += "return ";
                    }
                    if let Some(implementation) = class_.methods.get(&method.name) {
                        c += &format!(
                            "_{}_{}(({}*)this",
                            to_snake_case(&implementation.class_),
                            method.name,
                            implementation.class_
                        );
                    } else if iface.default_bodies.contains_key(&method.name) {
                        c += &format!("_{}_{}(this", iface.snake_name, method.name);
                    } else {
                        eprintln!(
                            "[ERROR] Class {} implements {} but does not provide '{}' and there is no default",
                            class_.name, iface_name, method.name
                        );
                        exit(1);
                    }
                    for argument in &method.arguments {
                        c += &format!(", {}", argument.name);
                    }
                    c += ");\n}\n\n";
                }
                c += &format!(
                    "static const {}Vtbl _{}{}Vtbl = {{\n",
                    iface_name, class_.name, iface_name
                );
                for method in iface.methods.values() {
                    c += &format!(
                        "    &_{}_{}_{}_vcall,\n",
                        class_.snake_name, iface.snake_name, method.name
                    );
                }
                c += "};\n\n";
            }

            c += &format!(
                "static const _InterfaceSlot _{}Interfaces[] = {{\n",
                class_.name
            );
            for iface_name in &class_.interface_names {
                c += &format!(
                    "    {{ _{}_ID, &_{}{}Vtbl }},\n",
                    iface_name, class_.name, iface_name
                );
            }
            c += "    { 0, NULL }\n";
            c += "};\n\n";
        }

        for method in class_.methods.values().filter(|method| method.is_virtual) {
            c += &format!(
                "static {} _{}_{}_vcall(void* this",
                method.return_type, class_.snake_name, method.name
            );
            for argument in &method.arguments {
                c += &format!(", {} {}", argument.type_, argument.name);
            }
            c += ") {\n    ";
            if method.return_type.trim() != "void" {
                c += "return ";
            }
            c += &format!(
                "_{}_{}(({}*)this",
                to_snake_case(&method.class_),
                method.name,
                method.class_
            );
            for argument in &method.arguments {
                c += &format!(", {}", argument.name);
            }
            c += ");\n}\n\n";
        }

        c += &format!("{}Vtbl _{}Vtbl = {{\n", class_.name, class_.name);
        if !class_.interface_names.is_empty() {
            c += &format!("    _{}Interfaces,\n", class_.name);
        } else {
            c += "    NULL,\n";
        }
        let mut current_class_name = String::new();
        for method in class_.methods.values() {
            if method.is_virtual {
                if method.origin_class != current_class_name {
                    c += &format!("    // {}\n", method.origin_class);
                    current_class_name = method.origin_class.clone();
                }
                c += &format!("    &_{}_{}_vcall,\n", class_.snake_name, method.name);
            }
        }
        c += "};\n\n";
        c
    }

    fn codegen_class_macros(&self, class_name: &str) -> String {
        let class_ = &self.classes[class_name];
        let mut c = String::new();
        for method in class_.methods.values() {
            if method.is_static {
                continue;
            }
            let return_cast = if method.is_return_self {
                format!("({}*)", class_.name)
            } else {
                String::new()
            };
            let target = if method.is_virtual {
                format!(
                    "(({class_name}*)(this))->vtbl->{}",
                    method.name,
                    class_name = class_.name
                )
            } else {
                format!("_{}_{}", to_snake_case(&method.class_), method.name)
            };
            c += &format!("#define {}_{}(", class_.snake_name, method.name);
            c += &iter::once("this".to_owned())
                .chain(method.arguments.iter().map(|a| a.name.clone()))
                .collect::<Vec<_>>()
                .join(", ");
            let receiver_type = if method.is_virtual {
                "void"
            } else {
                method.class_.as_str()
            };
            c += &format!(") {return_cast}{target}(({receiver_type}*)(this)");
            for argument in &method.arguments {
                let found_class = self
                    .classes
                    .values()
                    .find(|oc| argument.type_.starts_with(&oc.name))
                    .map(|oc| oc.name.clone());
                if let Some(cn) = found_class {
                    c += &format!(", ({}*)({})", cn, argument.name);
                } else {
                    c += &format!(", ({})", argument.name);
                }
            }
            c += ")\n";
        }
        c += "\n";
        c
    }

    fn convert_class(
        &mut self,
        is_header: bool,
        class_name: &str,
        supers_raw: Option<&str>,
        contents: &str,
    ) -> String {
        let (cn, parent_cn) = self.index_class(class_name, supers_raw, contents);
        let g = self.codegen_missing_methods(&cn, &parent_cn, is_header);

        let mut c = self.codegen_class_struct(&cn);
        c += &self.codegen_class_forward_decls(&cn);
        if !is_header && !self.classes[&cn].is_abstract {
            c += &self.codegen_class_vtbl_instance(&cn);
        }
        c += &self.codegen_class_macros(&cn);
        if !is_header {
            c += &g;
        }
        c
    }

    // MARK: Convert method / super calls
    fn convert_method(&self, caps: &Captures) -> String {
        let return_type = caps[1].to_owned();
        let class_name = caps[2].to_owned();
        let method_name = caps[3].to_owned();
        let arguments = caps[4].to_owned();

        if !self.classes.contains_key(&class_name) {
            eprintln!("[ERROR] Can't find class: {class_name}");
            exit(1);
        }
        let class_ = &self.classes[&class_name];
        let method = match class_.methods.get(&method_name) {
            Some(m) => m,
            None => {
                eprintln!("[ERROR] Can't find method: {class_name}::{method_name}");
                exit(1);
            }
        };
        let mut return_type = return_type;
        if method.is_return_self {
            return_type = return_type.replace("Self", &class_.name);
        }
        let mut c;
        if method.is_static {
            let args_str = if arguments.trim().is_empty() {
                "void".to_owned()
            } else {
                arguments.clone()
            };
            c = format!(
                "{} {}_{}({}) {{",
                return_type.trim(),
                class_.snake_name,
                method_name,
                args_str
            );
        } else {
            c = format!(
                "{} _{}_{}({}* this{}) {{",
                return_type.trim(),
                class_.snake_name,
                method_name,
                class_name,
                if !arguments.is_empty() {
                    format!(", {arguments}")
                } else {
                    String::new()
                }
            );
        }
        if method_name == "init" {
            for field in class_.fields.values() {
                if field.class_ == class_name
                    && let Some(ref default) = field.default
                {
                    c += &format!("\n    this->{} = {};", field.name, default);
                }
            }
        }
        if method_name == "deinit" {
            for field in class_.fields.values() {
                if field.class_ == class_name && field.attributes.contains_key("deinit") {
                    let deinit_attrs = &field.attributes["deinit"];
                    if !deinit_attrs.is_empty() {
                        c += &format!("\n    {}(this->{});", deinit_attrs[0], field.name);
                    } else {
                        c += &format!("\n    free(this->{});", field.name);
                    }
                }
            }
        }
        c
    }

    fn convert_super_call(&self, caps: &Captures) -> String {
        let parent_class_name = caps[1].to_owned();
        let method_name = caps[2].to_owned();
        let arguments = caps[3].to_owned();

        if !self.classes.contains_key(&parent_class_name) {
            eprintln!("[ERROR] Can't find class: {parent_class_name}");
            exit(1);
        }
        let parent_class = &self.classes[&parent_class_name];
        let method = match parent_class.methods.get(&method_name) {
            Some(m) => m,
            None => {
                eprintln!("[ERROR] Can't find method: {parent_class_name}::{method_name}");
                exit(1);
            }
        };
        format!(
            "_{}_{}(({}*)(this){});",
            to_snake_case(&method.class_),
            method.name,
            method.class_,
            if !arguments.is_empty() {
                format!(", {arguments}")
            } else {
                String::new()
            }
        )
    }

    // MARK: Transpile steps
    fn step_prelude_and_includes(&mut self, path: &str, is_header: bool, text: &str) -> String {
        let re_pragma_once = regex!(r"#pragma once\n");
        let re_include = regex!(r#"#include\s*["<](.+)\.hh[">]"#);
        let re_literal_string = regex!(r#"@"([^"]*)""#);
        let re_literal_bool = regex!(r"@(true|false)\b");
        let re_literal_float = regex!(r"@([0-9]+\.[0-9]+)");
        let re_literal_int = regex!(r"@([0-9]+)");
        let mut text = if !is_header {
            format!("// @generated\n#include \"prelude.h\"\n#include \"Object.hh\"\n{text}")
        } else {
            text.to_owned()
        };
        text = re_pragma_once.replace_all(&text, "").into_owned();
        // Inline convert_include using replace loop (can't use closure with &mut self)
        while let Some(caps) = re_include.captures(&text) {
            let start = caps.get(0).expect("group 0 always present").start();
            let end = caps.get(0).expect("group 0 always present").end();
            let include_name = caps[1].to_owned();
            drop(caps);
            let replacement = self.convert_include(path, &include_name);
            text = format!("{}{}{}", &text[..start], replacement, &text[end..]);
        }
        text = re_literal_string
            .replace_all(&text, |caps: &Captures| {
                format!("string_new(\"{}\")", &caps[1])
            })
            .into_owned();
        text = re_literal_bool
            .replace_all(&text, |caps: &Captures| format!("bool_new({})", &caps[1]))
            .into_owned();
        text = re_literal_float
            .replace_all(&text, |caps: &Captures| format!("float_new({})", &caps[1]))
            .into_owned();
        text = re_literal_int
            .replace_all(&text, |caps: &Captures| format!("int_new({})", &caps[1]))
            .into_owned();
        text
    }

    fn step_interfaces(&mut self, text: &str) -> String {
        let re_interface =
            regex!(r"class\s+(I[A-Z][_A-Za-z0-9]*)(\s*:\s*[_A-Za-z][_A-Za-z0-9,\s]*)?\s*\{");
        let mut text = text.to_owned();
        loop {
            let (match_start, match_end, iface_name, supers_raw) = {
                let Some(caps) = re_interface.captures(&text) else {
                    break;
                };
                let m0 = caps.get(0).expect("group 0 always present");
                (
                    m0.start(),
                    m0.end(),
                    caps[1].to_owned(),
                    caps.get(2).map(|m| m.as_str().to_owned()),
                )
            };
            let start = match_end - 1;
            let pos = find_matching_close(&text, start);
            let body = text[start + 1..pos].to_owned();
            let mut end = pos + 1;
            if end < text.len() && text.as_bytes()[end] == b';' {
                end += 1;
            }
            let replacement = self.convert_interface(&iface_name, supers_raw.as_deref(), &body);
            text = format!("{}{}{}", &text[..match_start], replacement, &text[end..]);
        }
        text
    }

    fn step_prescan_default_bodies(&mut self, text: &str) {
        let re_default_method = regex!(
            r"[_A-Za-z][_A-Za-z0-9 ]*[\**|\s+]\s*([_A-Za-z][_A-Za-z0-9]*)::([_A-Za-z][_A-Za-z0-9]*)\("
        );
        let default_methods: Vec<(String, String)> = re_default_method
            .captures_iter(text)
            .filter_map(|caps| {
                let iface_name = &caps[1];
                let method_name = &caps[2];
                self.interfaces
                    .get(iface_name)
                    .filter(|iface| iface.methods.contains_key(method_name))?;
                Some((iface_name.to_owned(), method_name.to_owned()))
            })
            .collect();
        for (iface_name, method_name) in default_methods {
            self.interfaces
                .get_mut(&iface_name)
                .expect("interface exists")
                .default_bodies
                .insert(method_name, String::new());
        }
    }

    fn replace_default_body_calls(interface: &Interface, body: &str, re_call: &Regex) -> String {
        let mut result = String::with_capacity(body.len());
        let mut last = 0;
        for caps in re_call.captures_iter(body) {
            let match_ = caps.get(0).expect("group 0 always present");
            let method_name = &caps[1];
            if interface.methods.contains_key(method_name) {
                result.push_str(&body[last..match_.start()]);
                result.push_str("_vtbl->");
                result.push_str(match_.as_str());
                last = match_.end();
            }
        }
        result.push_str(&body[last..]);
        result
    }

    fn step_classes(&mut self, text: &str, is_header: bool) -> String {
        let re_class_fwd = regex!(r"class\s+([_A-Za-z][_A-Za-z0-9]*)\s*;");
        let re_class =
            regex!(r"class\s+([_A-Za-z][_A-Za-z0-9]*)(\s*:\s*[_A-Za-z][_A-Za-z0-9,\s]*)?\s*\{");
        let mut text = re_class_fwd
            .replace_all(text, "typedef struct $1 $1;")
            .into_owned();
        loop {
            let (match_start, match_end, class_name, supers_raw) = {
                let Some(caps) = re_class.captures(&text) else {
                    break;
                };
                let m0 = caps.get(0).expect("group 0 always present");
                (
                    m0.start(),
                    m0.end(),
                    caps[1].to_owned(),
                    caps.get(2).map(|m| m.as_str().to_owned()),
                )
            };
            let start = match_end - 1;
            let pos = find_matching_close(&text, start);
            let body = text[start + 1..pos].to_owned();
            let mut end = pos + 1;
            if end < text.len() && text.as_bytes()[end] == b';' {
                end += 1;
            }
            let replacement =
                self.convert_class(is_header, &class_name, supers_raw.as_deref(), &body);
            text = format!("{}{}{}", &text[..match_start], replacement, &text[end..]);
        }
        text
    }

    fn step_default_body_implementations(&mut self, text: &str) -> String {
        let re_default_method = regex!(
            r"([_A-Za-z][_A-Za-z0-9 ]*[\**|\s+])\s*([_A-Za-z][_A-Za-z0-9]*)::([_A-Za-z][_A-Za-z0-9]*)\(([^\)]*)\)\s*\{"
        );
        let re_call = regex!(r"\b([_A-Za-z][_A-Za-z0-9]*)\(this\b");
        let mut text = text.to_owned();
        loop {
            let Some((dm_start, dm_end, ret_type, cur_iface_name, method_name, arguments_str)) =
                re_default_method.captures_iter(&text).find_map(|caps| {
                    let iface_name = &caps[2];
                    self.interfaces.contains_key(iface_name).then(|| {
                        let match_ = caps.get(0).expect("group 0 always present");
                        (
                            match_.start(),
                            match_.end(),
                            caps[1].to_owned(),
                            iface_name.to_owned(),
                            caps[3].to_owned(),
                            caps[4].to_owned(),
                        )
                    })
                })
            else {
                break;
            };
            {
                if !self.interfaces[&cur_iface_name]
                    .methods
                    .contains_key(&method_name)
                {
                    eprintln!("[ERROR] Interface {cur_iface_name} has no method '{method_name}'");
                    exit(1);
                }
                let dstart = dm_end - 1;
                let dpos = find_matching_close(&text, dstart);
                let dend = dpos + 1;
                let body_text = text[dstart + 1..dpos].to_owned();
                let def_arguments = parse_arguments(&arguments_str);

                let snake_iface = self.interfaces[&cur_iface_name].snake_name.clone();
                let mut fn_code = format!(
                    "static {} _{}_{}(void* this",
                    ret_type.trim(),
                    snake_iface,
                    method_name
                );
                for arg in &def_arguments {
                    fn_code += &format!(", {} {}", arg.type_, arg.name);
                }
                fn_code += ") {\n";
                fn_code += &format!("    const {cur_iface_name}Vtbl* _vtbl;\n");
                fn_code += "    {\n";
                fn_code += "        const _InterfaceSlot* _s = *(const _InterfaceSlot* const*)*(void* const*)this;\n";
                fn_code += "        _vtbl = NULL;\n";
                fn_code += "        if (_s) for (; _s->id; _s++) {\n";
                fn_code += &format!(
                    "            if (_s->id == _{cur_iface_name}_ID) {{ _vtbl = (const {cur_iface_name}Vtbl*)_s->vtbl; break; }}\n"
                );
                fn_code += "        }\n";
                fn_code += "    }\n";
                let transformed_body = Self::replace_default_body_calls(
                    &self.interfaces[&cur_iface_name],
                    &body_text,
                    re_call,
                );
                fn_code += &transformed_body;
                fn_code += "}\n\n";

                self.interfaces
                    .get_mut(&cur_iface_name)
                    .expect("interface exists")
                    .default_bodies
                    .insert(method_name.clone(), fn_code.clone());
                text = format!("{}{}{}", &text[..dm_start], fn_code, &text[dend..]);
            }
        }
        text
    }

    fn step_methods_and_super_calls(&self, text: &str) -> String {
        let re_method_def = regex!(
            r"([_A-Za-z][_A-Za-z0-9 ]*[\**|\s+])\s*([_A-Za-z][_A-Za-z0-9]*)::([_A-Za-z][_A-Za-z0-9]*)\(([^\)]*)\)\s*\{"
        );
        let re_super_call =
            regex!(r"([_A-Za-z][_A-Za-z0-9]*)::([_A-Za-z][_A-Za-z0-9]*)\(([^\)]*)\)\s*;");
        // Build both replacements in one pass per pattern.
        let text = {
            let mut result = String::new();
            let mut last = 0;
            for caps in re_method_def.captures_iter(text) {
                let m = caps.get(0).expect("group 0 always present");
                result.push_str(&text[last..m.start()]);
                result.push_str(&self.convert_method(&caps));
                last = m.end();
            }
            result.push_str(&text[last..]);
            result
        };
        {
            let mut result = String::new();
            let mut last = 0;
            for caps in re_super_call.captures_iter(&text) {
                let m = caps.get(0).expect("group 0 always present");
                result.push_str(&text[last..m.start()]);
                result.push_str(&self.convert_super_call(&caps));
                last = m.end();
            }
            result.push_str(&text[last..]);
            result
        }
    }

    fn step_for_in(&self, text: &str) -> String {
        let re_for_in = regex!(
            r"for\s*\(\s*([_A-Za-z][_A-Za-z0-9 \*]*\*?)\s+([_A-Za-z][_A-Za-z0-9]*)\s+in\s+([^\)]+)\)\s*\{"
        );
        let mut text = text.to_owned();
        let mut counter = 0usize;
        loop {
            let (match_start, match_end, var_type, var_name, iterable_expr) = {
                let Some(caps) = re_for_in.captures(&text) else {
                    break;
                };
                let m0 = caps.get(0).expect("group 0 always present");
                (
                    m0.start(),
                    m0.end(),
                    caps[1].trim().to_owned(),
                    caps[2].trim().to_owned(),
                    caps[3].trim().to_owned(),
                )
            };
            let iter_var = format!("_iter_{counter}");
            counter += 1;
            let fstart = match_end - 1;
            let fpos = find_matching_close(&text, fstart);
            let body = text[fstart + 1..fpos].trim().to_owned();
            let fend = fpos + 1;
            let replacement = format!(
                "{{\n    IIterator {iter_var} = i_iterable_iterator(cast<IIterable>({iterable_expr}));\n    while (i_iterator_has_next({iter_var})) {{\n        {var_type} {var_name} = ({var_type})i_iterator_next({iter_var});\n        {body}\n    }}\n    object_free((Object*){iter_var}.obj);\n}}",
            );
            text = format!("{}{}{}", &text[..match_start], replacement, &text[fend..]);
        }
        text
    }

    fn step_cast(&self, text: &str) -> String {
        let re_cast_scan = regex!(r"cast<([_A-Za-z][_A-Za-z0-9]*)>");
        let re_cast = regex!(r"cast<([_A-Za-z][_A-Za-z0-9]*)>\(");
        let re_main = regex!(r"\bint\s+main\s*\(");
        let re_insert_after_close = regex!(r"(}\n\n)([a-zA-Z#])");
        let mut text = text.to_owned();
        let mut ifaces_used: Vec<String> = Vec::new();
        for caps in re_cast_scan.captures_iter(&text) {
            let name = caps[1].to_owned();
            if self.interfaces.contains_key(&name) && !ifaces_used.contains(&name) {
                ifaces_used.push(name);
            }
        }

        if !ifaces_used.is_empty() {
            let mut lookup_code = String::new();
            for lk_iface_name in &ifaces_used {
                lookup_code +=
                    &format!("static {lk_iface_name} _cast_{lk_iface_name}(void* obj) {{\n");
                lookup_code += "    Object* _obj = (Object*)obj;\n";
                lookup_code += "    const _InterfaceSlot* s = _obj->vtbl->interfaces;\n";
                lookup_code += &format!("    const {lk_iface_name}Vtbl* vtbl = NULL;\n");
                lookup_code += "    if (s) for (; s->id; s++) {\n";
                lookup_code += &format!(
                    "        if (s->id == _{lk_iface_name}_ID) {{ vtbl = (const {lk_iface_name}Vtbl*)s->vtbl; break; }}\n"
                );
                lookup_code += "    }\n";
                lookup_code +=
                    &format!("    return ({lk_iface_name}){{ .obj = _obj, .vtbl = vtbl }};\n");
                lookup_code += "}\n\n";
            }
            let ins_pos = if let Some(m) = re_main.find(&text) {
                m.start()
            } else if let Some(m) = re_insert_after_close.find(&text) {
                m.end() - 1
            } else {
                text.len()
            };
            text = format!("{}{}{}", &text[..ins_pos], lookup_code, &text[ins_pos..]);
        }

        loop {
            let (match_start, match_end, cast_iface_name) = {
                let Some(caps) = re_cast.captures(&text) else {
                    break;
                };
                let iface = caps[1].to_owned();
                if !self.interfaces.contains_key(&iface) {
                    break;
                }
                let m0 = caps.get(0).expect("group 0 always present");
                (m0.start(), m0.end(), iface)
            };
            let cstart = match_end - 1;
            let cpos = find_matching_close(&text, cstart);
            let obj_expr = text[cstart + 1..cpos].trim().to_owned();
            let cend = cpos + 1;
            text = format!(
                "{}_cast_{}((void*)({})){}",
                &text[..match_start],
                cast_iface_name,
                obj_expr,
                &text[cend..]
            );
        }
        text
    }

    fn step_instanceof(&self, text: &str) -> String {
        let re_instanceof_scan = regex!(r"instanceof<([_A-Za-z][_A-Za-z0-9]*)>");
        let re_instanceof = regex!(r"instanceof<([_A-Za-z][_A-Za-z0-9]*)>\(");
        let re_main = regex!(r"\bint\s+main\s*\(");
        let re_func_marker = regex!(r"\n(?:typedef|extern|#|struct|}\s*;)");
        let re_func_impl = regex!(r"\n[_A-Za-z*][_A-Za-z0-9*\s]*\s+\**[_A-Za-z][_A-Za-z0-9]*\s*\(");
        let mut text = text.to_owned();
        let mut types_for_instanceof: Vec<String> = Vec::new();
        for caps in re_instanceof_scan.captures_iter(&text) {
            let type_name = caps[1].to_owned();
            if !types_for_instanceof.contains(&type_name) {
                types_for_instanceof.push(type_name);
            }
        }

        if !types_for_instanceof.is_empty() {
            let mut instanceof_code = String::new();
            let mut declared_vtbls: Vec<String> = Vec::new();
            for type_name in &types_for_instanceof {
                if self.classes.contains_key(type_name.as_str()) {
                    for sub in self.concrete_subclasses(type_name) {
                        if !declared_vtbls.contains(&sub) {
                            instanceof_code += &format!("typedef struct {sub}Vtbl {sub}Vtbl;\n");
                            instanceof_code += &format!("extern {sub}Vtbl _{sub}Vtbl;\n");
                            declared_vtbls.push(sub);
                        }
                    }
                }
            }
            instanceof_code += "\n";

            for type_name in &types_for_instanceof {
                if self.interfaces.contains_key(type_name.as_str()) {
                    instanceof_code +=
                        &format!("static bool _instanceof_{type_name}(void* obj) {{\n");
                    instanceof_code += "    Object* _obj = (Object*)obj;\n";
                    instanceof_code += "    const _InterfaceSlot* s = _obj->vtbl->interfaces;\n";
                    instanceof_code += "    if (!s) return false;\n";
                    instanceof_code += "    for (; s->id; s++)\n";
                    instanceof_code +=
                        &format!("        if (s->id == _{type_name}_ID) return true;\n");
                    instanceof_code += "    return false;\n";
                    instanceof_code += "}\n\n";
                } else if self.classes.contains_key(type_name.as_str()) {
                    let subs = self.concrete_subclasses(type_name);
                    instanceof_code +=
                        &format!("static bool _instanceof_{type_name}(void* obj) {{\n");
                    instanceof_code += "    Object* _obj = (Object*)obj;\n";
                    if !subs.is_empty() {
                        let checks = subs
                            .iter()
                            .map(|s| format!("_obj->vtbl == (void*)&_{s}Vtbl"))
                            .collect::<Vec<_>>()
                            .join(" ||\n        ");
                        instanceof_code += &format!("    return {checks};\n");
                    } else {
                        instanceof_code += "    return false;\n";
                    }
                    instanceof_code += "}\n\n";
                } else {
                    eprintln!(
                        "[WARNING] Type '{type_name}' used in instanceof<> is not defined as a class or interface"
                    );
                }
            }

            let ins_pos2 = if let Some(m) = re_main.find(&text) {
                m.start()
            } else if let Some(fm) = re_func_marker.find(&text) {
                let rest = &text[fm.end()..];
                if let Some(fi) = re_func_impl.find(rest) {
                    fm.end() + fi.start()
                } else {
                    text.len()
                }
            } else {
                text.len()
            };
            text = format!(
                "{}{}{}",
                &text[..ins_pos2],
                instanceof_code,
                &text[ins_pos2..]
            );
        }

        loop {
            let (match_start, match_end, inst_type_name) = {
                let Some(caps) = re_instanceof.captures(&text) else {
                    break;
                };
                let m0 = caps.get(0).expect("group 0 always present");
                (m0.start(), m0.end(), caps[1].to_owned())
            };
            if !self.interfaces.contains_key(&inst_type_name)
                && !self.classes.contains_key(&inst_type_name)
            {
                eprintln!(
                    "[ERROR] Type '{inst_type_name}' used in instanceof<> is not defined as a class or interface"
                );
                text = format!("{}false{}", &text[..match_start], &text[match_end..]);
                continue;
            }
            let istart = match_end - 1;
            let ipos = find_matching_close(&text, istart);
            let inst_expr = text[istart + 1..ipos].trim().to_owned();
            let iend = ipos + 1;
            text = format!(
                "{}_instanceof_{}({}){}",
                &text[..match_start],
                inst_type_name,
                inst_expr,
                &text[iend..]
            );
        }
        text
    }

    /// Translates one source or header file into C11 source text.
    pub fn transpile(&mut self, path: &str, is_header: bool, text: &str) -> String {
        let text = self.step_prelude_and_includes(path, is_header, text);
        let text = self.step_interfaces(&text);
        if !is_header {
            self.step_prescan_default_bodies(&text);
        }
        let text = self.step_classes(&text, is_header);
        if !is_header {
            let text = self.step_default_body_implementations(&text);
            let text = self.step_methods_and_super_calls(&text);
            let text = self.step_for_in(&text);
            let text = self.step_cast(&text);
            let text = self.step_instanceof(&text);
            return text;
        }
        text
    }
}
