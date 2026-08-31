//! This module exposes a [`ModuleSearch`] abstraction, which encapsulates reusable
//! search state for namespace-aware module enumeration, and is equally usable for
//! ordinary, single module resolution.
//!
//! [`ModuleSearch`] works by providing an interface that describes traversal of the
//! components of a module name
//!
//! - [`ModuleSearch::enter_package`] returns search state for resolving descendants of a module prefix
//! - [`ModuleSearch::resolve_child`] selects the module candidates for a particular terminal component
//!   of a module name, while leaving the search state reusable for resolving a different child with
//!   the same module name prefix.

use std::cell::OnceCell;
use std::rc::Rc;

use crate::module_name::ModuleName;

use super::{
    ComponentFileFilter, ModuleResolutionCandidate, NameResolver, ResolvedNames, StubPackagePaths,
    normalize_candidates, search_paths, stub_package_index,
};

/// A cursor that retains search state for a module name prefix, allowing module enumeration to
/// resolve several children using ordinary import resolution rules.
pub(super) struct ModuleSearch<'resolver, 'db> {
    resolver: &'resolver NameResolver<'db>,
    state: SearchState<'db>,
}

impl<'resolver, 'db> ModuleSearch<'resolver, 'db> {
    pub(super) fn new(resolver: &'resolver NameResolver<'db>) -> Self {
        Self {
            resolver,
            state: SearchState::Root,
        }
    }

    /// Returns search state for resolving descendants of the package named by the prefix stored
    /// in [`Self::state`] and the given component.
    pub(super) fn enter_package(&self, component: &str) -> Option<Self> {
        let prefix = self.child_name(component)?;

        let candidates = match &self.state {
            SearchState::Root if self.resolver.context.mode.is_typing() => {
                SearchCandidates::Typing(TypingCandidates::for_first_component(
                    self.resolver,
                    component,
                ))
            }
            SearchState::Root => SearchCandidates::Runtime(RuntimeCandidates::for_first_component(
                self.resolver,
                component,
            )),
            SearchState::Prefix {
                candidates: SearchCandidates::Typing(typing),
                ..
            } => SearchCandidates::Typing(typing.enter_package(self.resolver, component)),
            SearchState::Prefix {
                candidates: SearchCandidates::Runtime(runtime),
                ..
            } => SearchCandidates::Runtime(runtime.enter_package(self.resolver, component)),
        };

        match &candidates {
            SearchCandidates::Typing(typing) if typing.is_empty(self.resolver, &prefix) => None,
            SearchCandidates::Runtime(runtime) if runtime.is_empty() => None,
            _ => Some(Self {
                resolver: self.resolver,
                state: SearchState::Prefix { prefix, candidates },
            }),
        }
    }

    /// Selects candidates for one immediate child using ordinary import resolution rules, treating
    /// the prefix and child component as a complete module name. Leaves the parent reusable for
    /// another child.
    pub(super) fn resolve_child(&self, component: &str) -> Option<ResolvedNames<'db>> {
        let name = self.child_name(component)?;
        let candidates = self.resolve_child_candidates(&name);
        (!candidates.is_empty()).then_some(candidates)
    }

    /// Selects candidates for one immediate child without converting them to a final module.
    fn resolve_child_candidates(&self, name: &ModuleName) -> ResolvedNames<'db> {
        let context = &self.resolver.context;
        match &self.state {
            SearchState::Prefix {
                prefix,
                candidates: SearchCandidates::Typing(typing),
            } => {
                return typing.resolve_child(self.resolver, prefix, name.last_component());
            }
            SearchState::Prefix {
                candidates: SearchCandidates::Runtime(runtime),
                ..
            } => {
                return runtime.resolve_child(self.resolver, name.last_component());
            }
            SearchState::Root => {}
        }

        // A top-level name such as `acme` has no parent prefix that could hide a stub overlay.
        // Typing resolution can therefore search all paths together.
        let stubs = if context.mode.is_typing() {
            stub_package_index(context.db, context.resolver_environment).all()
        } else {
            StubPackagePaths::default()
        };

        let candidates = self.resolver.discover_roots(
            name.first_component(),
            context.mode.is_non_shadowable(
                context
                    .resolver_environment
                    .python_version(context.db)
                    .minor,
                name.as_str(),
            ),
            search_paths(context.db, context.resolver_environment, context.mode),
            stubs,
        );

        normalize_candidates(context.db, candidates, false)
    }

    // Returns the full module name of the given component (by appending the
    // component to the stored module name prefix).
    fn child_name(&self, component: &str) -> Option<ModuleName> {
        let child = ModuleName::new(component)?;
        match &self.state {
            SearchState::Root => Some(child),
            SearchState::Prefix { prefix, .. } => {
                let mut name = prefix.clone();
                name.extend(&child);
                Some(name)
            }
        }
    }
}

/// Search state before the first component, or for a module name prefix.
enum SearchState<'db> {
    Root,
    Prefix {
        prefix: ModuleName,
        candidates: SearchCandidates<'db>,
    },
}

/// Candidates retained for a module name prefix under the selected resolution mode.
enum SearchCandidates<'db> {
    Typing(TypingCandidates<'db>),
    Runtime(RuntimeCandidates<'db>),
}

/// Candidates retained in the search state for a module name prefix in typing mode.
///
/// Stub overlays and the full search across all paths retain separate candidates for the same
/// prefix. The candidates from all paths are computed when needed and reused for resolving siblings.
///
/// For example, consider these files, with `extra` configured as an extra search path:
///
/// ```text
/// extra
/// └── acme
///     └── patched.pyi
/// site-packages
/// ├── acme-stubs
/// │   ├── __init__.pyi
/// │   ├── py.typed          # contains "partial"
/// │   └── stubbed.pyi
/// └── acme
///     ├── __init__.py
///     ├── patched.py
///     ├── stubbed.py
///     └── source_only.py
/// ```
///
/// Typing resolution first tries extra-path stub overlays when resolving a submodule.
///   Here, `acme.patched` resolves to `extra/acme/patched.pyi`. While following a dotted name,
///   the overlay search may traverse namespace packages or packages defined by `__init__.py`
///   for the preceding components (i.e., `acme`). It succeeds only if the requested module
///   (`acme.patched`) is defined by a `.pyi` file; finding only `patched.py` would not suffice.
///   If no overlay supplies the submodule, a full search considers stubs and runtime modules
///   under typing precedence: `acme.stubbed` resolves to `acme-stubs/stubbed.pyi`, while
///   `acme.source_only` resolves to `acme/source_only.py`[^1].
///
/// [^1]: The `partial` marker allows the full search to use runtime modules missing from the
/// stub package. Without this marker, the stub package is treated as complete, so
/// `acme.source_only` would not resolve.
struct TypingCandidates<'db> {
    /// Candidates for the current prefix, searched separately so candidates from other search
    /// roots cannot shadow them.
    overlay_candidates: ResolvedNames<'db>,
    /// Candidates for the first component from extra paths, before applying precedence or
    /// traversing the remaining components.
    ///
    /// The search across all paths combines these with candidates from the other search roots
    /// before applying precedence. Keep the originals to avoid probing extra paths again; share
    /// them across cursors to avoid copying the same starting point at each depth. `None` means
    /// no extra-path candidates were found.
    candidates_from_extra_paths: Option<Rc<ResolvedNames<'db>>>,
    /// Candidates for the current prefix from all paths, including PEP 561 stub packages.
    ///
    /// Compute these lazily so overlay-only resolution need not probe other paths, and reuse
    /// them across sibling lookups.
    full_search_candidates: OnceCell<ResolvedNames<'db>>,
}

impl<'db> TypingCandidates<'db> {
    fn for_first_component(resolver: &NameResolver<'db>, component: &str) -> Self {
        let context = &resolver.context;
        let (stubs, _) =
            stub_package_index(context.db, context.resolver_environment).split_overlay();
        let candidates = resolver.discover_roots(
            component,
            // This is a parent prefix: a local `types` package may supply
            // `types.child`, even though resolving `types` itself selects stdlib.
            false,
            search_paths(context.db, context.resolver_environment, context.mode)
                .take_while(|path| path.is_extra()),
            stubs,
        );
        Self {
            overlay_candidates: normalize_candidates(context.db, candidates.clone(), true),
            candidates_from_extra_paths: (!candidates.is_empty()).then(|| Rc::new(candidates)),
            full_search_candidates: OnceCell::new(),
        }
    }

    fn enter_package(&self, resolver: &NameResolver<'db>, component: &str) -> Self {
        let overlay_candidates = resolver.advance_candidates(
            self.overlay_candidates.clone(),
            component,
            ComponentFileFilter::ByMode,
            true,
        );
        // Reuse candidates from all paths if already computed. Otherwise, keep that part of the
        // search state lazy while following an overlay.
        let full_search_candidates = self.full_search_candidates.get().map(|candidates| {
            resolver.advance_candidates(
                candidates.clone(),
                component,
                ComponentFileFilter::ByMode,
                true,
            )
        });
        Self {
            overlay_candidates,
            candidates_from_extra_paths: self.candidates_from_extra_paths.as_ref().map(Rc::clone),
            full_search_candidates: full_search_candidates
                .map(OnceCell::from)
                .unwrap_or_default(),
        }
    }

    /// Tries stub overlays first, then the search across all paths including stub packages.
    fn resolve_child(
        &self,
        resolver: &NameResolver<'db>,
        prefix: &ModuleName,
        component: &str,
    ) -> ResolvedNames<'db> {
        let overlay = resolver.advance_candidates(
            self.overlay_candidates.clone(),
            component,
            // When resolving `acme.tools`, the overlay search may have entered `acme` through
            // `acme/__init__.py`. The requested child must now come from `tools.pyi` or
            // `tools/__init__.pyi`, not a runtime `.py` file.
            ComponentFileFilter::StubOnly,
            false,
        );
        if !overlay.is_empty() {
            return overlay;
        }

        resolver.advance_candidates(
            self.full_search_candidates(resolver, prefix).to_vec(),
            component,
            ComponentFileFilter::ByMode,
            false,
        )
    }

    fn is_empty(&self, resolver: &NameResolver<'db>, prefix: &ModuleName) -> bool {
        // An overlay may supply descendants without probing the rest of the search paths.
        self.overlay_candidates.is_empty()
            && self.full_search_candidates(resolver, prefix).is_empty()
    }

    /// Establishes candidates for the current prefix across all search paths using ordinary
    /// typing resolution rules. Retains them for subsequent child resolution.
    fn full_search_candidates(
        &self,
        resolver: &NameResolver<'db>,
        prefix: &ModuleName,
    ) -> &[ModuleResolutionCandidate<'db>] {
        let context = &resolver.context;
        self.full_search_candidates.get_or_init(|| {
            let (_, stubs) =
                stub_package_index(context.db, context.resolver_environment).split_overlay();
            // Combine candidates for the first component before traversing its descendants:
            // a package found in one search root can shadow candidates found in another.
            // Appending candidates at the current prefix would miss that shadowing.
            let mut candidates = self
                .candidates_from_extra_paths
                .as_deref()
                .cloned()
                .unwrap_or_default();
            candidates.extend(
                resolver.discover_roots(
                    prefix.first_component(),
                    // This is a parent prefix: a local `types` package may supply
                    // `types.child`, even though resolving `types` itself selects stdlib.
                    false,
                    search_paths(context.db, context.resolver_environment, context.mode)
                        .skip_while(|path| path.is_extra()),
                    stubs,
                ),
            );
            candidates = normalize_candidates(context.db, candidates, true);
            for component in prefix.components().skip(1) {
                candidates = resolver.advance_candidates(
                    candidates,
                    component,
                    ComponentFileFilter::ByMode,
                    true,
                );
            }
            candidates
        })
    }
}

/// Candidates retained in the search state for a module name prefix in runtime mode.
struct RuntimeCandidates<'db> {
    candidates: ResolvedNames<'db>,
}

impl<'db> RuntimeCandidates<'db> {
    fn for_first_component(resolver: &NameResolver<'db>, component: &str) -> Self {
        let context = &resolver.context;
        let candidates = resolver.discover_roots(
            component,
            // This is a parent prefix: a local `types` package may supply
            // `types.child`, even though resolving `types` itself selects stdlib.
            false,
            search_paths(context.db, context.resolver_environment, context.mode),
            StubPackagePaths::default(),
        );

        Self {
            candidates: normalize_candidates(context.db, candidates, true),
        }
    }

    fn enter_package(&self, resolver: &NameResolver<'db>, component: &str) -> Self {
        Self {
            candidates: resolver.advance_candidates(
                self.candidates.clone(),
                component,
                ComponentFileFilter::ByMode,
                true,
            ),
        }
    }

    /// Selects candidates from runtime files only.
    fn resolve_child(&self, resolver: &NameResolver<'db>, component: &str) -> ResolvedNames<'db> {
        resolver.advance_candidates(
            self.candidates.clone(),
            component,
            ComponentFileFilter::ByMode,
            false,
        )
    }

    fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use ruff_db::Db as _;
    use ruff_db::system::{DbWithWritableSystem, SystemPath, SystemPathBuf};

    use crate::db::tests::TestDb;
    use crate::resolve::ModuleResolveMode;
    use crate::settings::SearchPathSettings;
    use crate::strategy::FallibleStrategy;
    use crate::testing::TestCaseBuilder;

    use super::{ModuleSearch, NameResolver};

    #[test]
    fn sibling_searches_reuse_split_namespace() {
        let db = search_db(
            &["/src/acme/reports.py", "/site-packages/acme/tools.py"],
            &[],
        );
        for mode in [ModuleResolveMode::Typing, ModuleResolveMode::Runtime] {
            let resolver = NameResolver::new(&db, db.resolver_environment(), mode);
            let root = ModuleSearch::new(&resolver);
            let acme = root.enter_package("acme").expect("namespace exists");
            assert_resolves_to(&db, &acme, "reports", "/src/acme/reports.py");
            assert_resolves_to(&db, &acme, "tools", "/site-packages/acme/tools.py");
            assert!(acme.resolve_child("missing").is_none());
            assert_resolves_to(&db, &acme, "reports", "/src/acme/reports.py");
        }
    }

    #[test]
    fn selected_initializer_preserves_legacy_namespace_search() {
        let mut db = search_db(
            &["/src/acme/reports.py", "/site-packages/acme/tools.py"],
            &[],
        );
        for initializer in ["/src/acme/__init__.py", "/site-packages/acme/__init__.py"] {
            db.write_file(
                initializer,
                r#"
__path__ = __import__("pkgutil").extend_path(__path__, __name__)
"#,
            )
            .expect("write legacy namespace initializer");
        }
        let resolver = NameResolver::new(&db, db.resolver_environment(), ModuleResolveMode::Typing);
        let root = ModuleSearch::new(&resolver);
        assert_resolves_to(&db, &root, "acme", "/src/acme/__init__.py");
        let acme = root.enter_package("acme").expect("legacy namespace exists");
        assert_resolves_to(&db, &acme, "reports", "/src/acme/reports.py");
        assert_resolves_to(&db, &acme, "tools", "/site-packages/acme/tools.py");
    }

    #[test]
    fn overlay_and_runtime_siblings_in_either_order() {
        let db = search_db(
            &[
                "/extra/acme/patched.pyi",
                "/src/acme/__init__.py",
                "/src/acme/patched.py",
                "/src/acme/runtime.py",
            ],
            &["/extra"],
        );
        for children in [["patched", "runtime"], ["runtime", "patched"]] {
            let resolver =
                NameResolver::new(&db, db.resolver_environment(), ModuleResolveMode::Typing);
            let acme = ModuleSearch::new(&resolver)
                .enter_package("acme")
                .expect("package has an overlay and runtime candidates");
            for child in children {
                let expected = match child {
                    "patched" => "/extra/acme/patched.pyi",
                    _ => "/src/acme/runtime.py",
                };
                assert_resolves_to(&db, &acme, child, expected);
            }
        }
    }

    fn search_db(paths: &[&str], extra_paths: &[&str]) -> TestDb {
        let mut db = TestCaseBuilder::new().build().db;
        db.write_files(paths.iter().map(|path| {
            (
                *path, r#"
"#,
            )
        }))
        .expect("write search fixtures");
        let settings = SearchPathSettings {
            src_roots: vec![SystemPathBuf::from("/src")],
            site_packages_paths: vec![SystemPathBuf::from("/site-packages")],
            custom_typeshed: Some(SystemPathBuf::from("/typeshed")),
            extra_paths: extra_paths
                .iter()
                .copied()
                .map(SystemPathBuf::from)
                .collect(),
            ..SearchPathSettings::empty()
        };
        db.set_search_paths(
            settings
                .to_search_paths(db.system(), db.vendored(), &FallibleStrategy)
                .expect("configure search fixtures"),
        );
        db
    }

    fn assert_resolves_to(db: &TestDb, search: &ModuleSearch, component: &str, expected: &str) {
        let name = search.child_name(component).expect("valid child name");
        let candidate = search
            .resolve_child(component)
            .and_then(|candidates| candidates.into_iter().next())
            .expect("child resolves");
        let module = candidate.into_module(db, db.resolver_environment(), &name);
        let file = module.file(db).expect("child has a defining file");
        assert_eq!(
            file.path(db).as_system_path(),
            Some(SystemPath::new(expected))
        );
    }
}
