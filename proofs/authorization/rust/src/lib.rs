#[path = "../../../../crates/veoveo-hub-sdk/src/registration/authorization_decision.rs"]
mod decision;

#[cfg(test)]
mod tests {
    use super::decision::{
        self, AllowedQuery, Cursor, Matcher, Path, ResourceView, Segment, TemplateView,
    };
    use std::cell::Cell;

    struct Items<'a, T> {
        values: std::vec::IntoIter<T>,
        calls: &'a Cell<usize>,
    }
    impl<T> Cursor<T> for Items<'_, T> {
        fn next(&mut self) -> Option<T> {
            self.calls.set(self.calls.get() + 1);
            self.values.next()
        }
    }
    struct Matched<'a>(&'a Cell<usize>);
    impl Matcher<bool> for Matched<'_> {
        fn matches(&self, matched: bool) -> bool {
            self.0.set(self.0.get() + 1);
            matched
        }
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Access {
        Authenticated,
        Permission(u8),
    }

    #[test]
    fn exact_access_overrides_conflicting_templates_without_visiting_them() {
        let visits = Cell::new(0);
        let matches = Cell::new(0);
        let mut items = Items {
            values: vec![(true, Access::Permission(1)), (true, Access::Permission(2))].into_iter(),
            calls: &visits,
        };
        assert_eq!(
            decision::resolve(Some(Access::Authenticated), &mut items, &Matched(&matches)),
            Some(Access::Authenticated)
        );
        assert_eq!((visits.get(), matches.get()), (0, 0));
    }

    #[test]
    fn every_raw_candidate_is_matched_and_first_conflict_short_circuits() {
        let visits = Cell::new(0);
        let matches = Cell::new(0);
        let mut items = Items {
            values: vec![
                (false, Access::Permission(7)),
                (true, Access::Authenticated),
                (true, Access::Permission(1)),
                (true, Access::Authenticated),
            ]
            .into_iter(),
            calls: &visits,
        };
        assert_eq!(
            decision::resolve(None, &mut items, &Matched(&matches)),
            None
        );
        assert_eq!((visits.get(), matches.get()), (3, 3));
    }

    #[test]
    fn zero_and_all_agree_cases_preserve_full_access_values() {
        let visits = Cell::new(0);
        let matches = Cell::new(0);
        for (values, expected) in [
            (vec![], None),
            (vec![(false, Access::Permission(1))], None),
            (
                vec![(true, Access::Authenticated), (true, Access::Authenticated)],
                Some(Access::Authenticated),
            ),
            (
                vec![(true, Access::Permission(1)), (true, Access::Permission(1))],
                Some(Access::Permission(1)),
            ),
            (
                vec![(true, Access::Permission(1)), (true, Access::Permission(2))],
                None,
            ),
        ] {
            let mut items = Items {
                values: values.into_iter(),
                calls: &visits,
            };
            assert_eq!(
                decision::resolve(None, &mut items, &Matched(&matches)),
                expected
            );
        }
    }

    impl Segment<u8> for u8 {
        fn is_empty(&self) -> bool {
            *self == 0
        }
        fn literal_eq(&self, literal: &u8) -> bool {
            self == literal
        }
    }
    impl AllowedQuery<u8> for Vec<u8> {
        fn is_empty(&self) -> bool {
            self.is_empty()
        }
        fn contains(&self, name: &u8) -> bool {
            self.as_slice().contains(name)
        }
    }
    fn match_path(
        path: &[Path<u8>],
        segments: &[u8],
        trailing: bool,
        present: bool,
        names: Vec<u8>,
        allowed: Vec<u8>,
    ) -> bool {
        let calls = Cell::new(0);
        let empty = names.is_empty();
        decision::matches_view(
            TemplateView {
                path,
                trailing_slash: trailing,
                query: &allowed,
            },
            ResourceView {
                segments,
                has_query: present,
                query_empty: empty,
                query: Items {
                    values: names.into_iter(),
                    calls: &calls,
                },
            },
        )
    }

    #[test]
    fn scalar_keeps_empty_segment_and_tail_accepts_zero_or_many() {
        assert!(match_path(
            &[Path::Scalar],
            &[0],
            false,
            false,
            vec![],
            vec![]
        ));
        assert!(!match_path(
            &[Path::Scalar],
            &[],
            false,
            false,
            vec![],
            vec![]
        ));
        assert!(match_path(&[Path::Tail], &[], false, false, vec![], vec![]));
        assert!(match_path(
            &[Path::Tail],
            &[1, 2],
            false,
            false,
            vec![],
            vec![]
        ));
        assert!(!match_path(
            &[Path::Literal(1), Path::Tail],
            &[],
            false,
            false,
            vec![],
            vec![]
        ));
        assert!(!match_path(
            &[Path::Literal(1)],
            &[2],
            false,
            false,
            vec![],
            vec![]
        ));
    }

    #[test]
    fn trailing_slash_and_query_checks_keep_native_semantics() {
        assert!(match_path(
            &[Path::Scalar],
            &[1, 0],
            true,
            false,
            vec![],
            vec![]
        ));
        assert!(!match_path(
            &[Path::Scalar],
            &[1],
            true,
            false,
            vec![],
            vec![]
        ));
        assert!(!match_path(&[], &[], false, true, vec![], vec![1]));
        assert!(!match_path(&[], &[], false, true, vec![1], vec![]));
        assert!(match_path(&[], &[], false, true, vec![2, 1], vec![1, 2]));
        assert!(!match_path(&[], &[], false, true, vec![3], vec![1, 2]));
        assert!(!decision::same_root(&1_u8, &2));
    }
}
