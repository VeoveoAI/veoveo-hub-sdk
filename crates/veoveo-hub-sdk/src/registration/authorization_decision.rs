//! Private access resolution and matching of native parsed views. Native parsing,
//! collections and decoded text stay at the adapter boundary.

pub(crate) trait Cursor<I> {
    fn next(&mut self) -> Option<I>;
}

pub(crate) trait Matcher<T> {
    fn matches(&self, template: T) -> bool;
}

/// Raw candidates retain their access value, including its original borrow.
/// Exact access does not consume a candidate or invoke the matcher.
pub(crate) fn resolve<T, A: PartialEq + Copy, C: Cursor<(T, A)>, M: Matcher<T>>(
    exact: Option<A>,
    candidates: &mut C,
    matcher: &M,
) -> Option<A> {
    if let Some(access) = exact {
        return Some(access);
    }
    let mut first = None;
    while let Some((template, access)) = candidates.next() {
        if matcher.matches(template) {
            match first {
                None => first = Some(access),
                Some(previous) if access != previous => return None,
                Some(_) => {}
            }
        }
    }
    first
}

// This is the native parser's existing private path representation, shared with
// its matcher rather than copied into a second owning template shape.
pub(crate) enum Path<L> {
    Literal(L),
    Scalar,
    Tail,
}

pub(crate) trait Segment<L> {
    fn is_empty(&self) -> bool;
    fn literal_eq(&self, literal: &L) -> bool;
}

pub(crate) trait AllowedQuery<N> {
    fn is_empty(&self) -> bool;
    fn contains(&self, name: &N) -> bool;
}

pub(crate) struct TemplateView<'a, L, Q> {
    pub path: &'a [Path<L>],
    pub trailing_slash: bool,
    pub query: &'a Q,
}

pub(crate) struct ResourceView<'a, S, C> {
    pub segments: &'a [S],
    pub has_query: bool,
    pub query_empty: bool,
    pub query: C,
}

pub(crate) fn same_root<K: PartialEq>(actual: &K, expected: &K) -> bool {
    actual == expected
}

pub(crate) fn path_matches<L, S: Segment<L>>(
    path: &[Path<L>],
    segments: &[S],
    trailing_slash: bool,
) -> bool {
    let mut active = segments.len();
    if trailing_slash {
        if active == 0 {
            return false;
        }
        if !segments[active - 1].is_empty() {
            return false;
        }
        active -= 1;
    }
    let path_len = path.len();
    let tail = path_len != 0 && matches!(&path[path_len - 1], Path::Tail);
    if tail {
        // A terminal Tail establishes path_len > 0. This is the same cardinality
        // test as active + 1 < path_len, without addition at the integer limit.
        if active < path_len - 1 {
            return false;
        }
    } else if active != path_len {
        return false;
    }
    let mut index = 0;
    while index < path_len && index < active {
        match &path[index] {
            Path::Literal(literal) if !segments[index].literal_eq(literal) => {
                return false;
            }
            _ => {}
        }
        index += 1;
    }
    true
}

pub(crate) fn matches_view<L, S: Segment<L>, N, C: Cursor<N>, Q: AllowedQuery<N>>(
    template: TemplateView<'_, L, Q>,
    mut resource: ResourceView<'_, S, C>,
) -> bool {
    if !path_matches(template.path, resource.segments, template.trailing_slash) {
        return false;
    }
    if resource.has_query && (template.query.is_empty() || resource.query_empty) {
        return false;
    }
    while let Some(name) = resource.query.next() {
        if !template.query.contains(&name) {
            return false;
        }
    }
    true
}
