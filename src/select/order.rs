//! ABOUTME: The ordering rule of `promote`: an item's rank is the place of the first promote tag it carries.
//! ABOUTME: Pure arithmetic over ranks and positions; the resume walk decides what a list is and applies it.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::Tag;

/// Where an item sorts in its list: the place in `promote` of the first tag it carries, else after every place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Rank {
    /// Smaller places sort first.
    Promoted(usize),
    /// Carries no promote tag; derived `Ord` puts it after every `Promoted`.
    Unpromoted,
}

/// The promote list as a lookup from tag to place; a repeated tag keeps its first place.
#[derive(Debug, Default)]
pub(super) struct Promotion<'a> {
    places: BTreeMap<&'a Tag, usize>,
}

impl<'a> Promotion<'a> {
    pub(super) fn new(promote: &'a [Tag]) -> Self {
        let mut places = BTreeMap::new();
        for (place, tag) in promote.iter().enumerate() {
            places.entry(tag).or_insert(place);
        }
        Self { places }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// The best rank any of `tags` gives; `Unpromoted` when none is in the list.
    pub(super) fn rank_of(&self, tags: &BTreeSet<Tag>) -> Rank {
        tags.iter()
            .filter_map(|t| self.places.get(t).copied())
            .min()
            .map_or(Rank::Unpromoted, Rank::Promoted)
    }
}

/// The new order of a list: its old positions, in the order they now stand. Equal ranks keep their order.
pub(super) fn permutation(ranks: &[Rank]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..ranks.len()).collect();
    // `sort_by_key` is stable, which is the whole of "equal ranks keep their order".
    order.sort_by_key(|&i| ranks[i]);
    order
}

/// `(from, to)`, 0-based, for each item that now stands higher than before, top first.
pub(super) fn risen(permutation: &[usize]) -> Vec<(usize, usize)> {
    permutation
        .iter()
        .enumerate()
        .filter(|&(to, &from)| to < from)
        .map(|(to, &from)| (from, to))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn tag(n: &str) -> Tag {
        assert_ok!(Tag::try_new(n))
    }

    fn tags(names: &[&str]) -> BTreeSet<Tag> {
        names.iter().map(|n| tag(n)).collect()
    }

    #[test]
    fn a_promoted_rank_sorts_before_an_unpromoted_one_and_earlier_places_first() {
        assert!(Rank::Promoted(0) < Rank::Promoted(1));
        assert!(Rank::Promoted(usize::MAX) < Rank::Unpromoted);
    }

    #[test]
    fn the_first_listed_place_among_an_items_tags_decides_its_rank() {
        let promote = [tag("people"), tag("lead"), tag("scale")];
        let promotion = Promotion::new(&promote);
        assert_eq!(promotion.rank_of(&tags(&["scale", "people"])), Rank::Promoted(0));
        assert_eq!(promotion.rank_of(&tags(&["scale"])), Rank::Promoted(2));
        assert_eq!(promotion.rank_of(&tags(&["arch"])), Rank::Unpromoted);
        assert_eq!(promotion.rank_of(&BTreeSet::new()), Rank::Unpromoted);
    }

    #[test]
    fn a_repeated_promote_tag_keeps_its_first_place() {
        let promote = [tag("lead"), tag("people"), tag("lead")];
        let promotion = Promotion::new(&promote);
        assert_eq!(promotion.rank_of(&tags(&["lead"])), Rank::Promoted(0));
        assert_eq!(promotion.rank_of(&tags(&["people"])), Rank::Promoted(1));
    }

    #[test]
    fn an_empty_promote_list_is_empty_and_ranks_everything_unpromoted() {
        let promotion = Promotion::new(&[]);
        assert!(promotion.is_empty());
        assert_eq!(promotion.rank_of(&tags(&["lead"])), Rank::Unpromoted);
    }

    #[test]
    fn permutation_keeps_the_order_of_equal_ranks() {
        use Rank::{Promoted, Unpromoted};
        let ranks = [Unpromoted, Promoted(1), Promoted(0), Unpromoted, Promoted(1)];
        assert_eq!(permutation(&ranks), vec![2, 1, 4, 0, 3]);
    }

    #[test]
    fn risen_lists_the_items_whose_new_position_is_smaller_and_none_for_the_identity() {
        assert_eq!(risen(&[2, 1, 4, 0, 3]), vec![(2, 0), (4, 2)]);
        assert_eq!(risen(&[0, 1, 2]), vec![]);
    }

    fn rank() -> impl Strategy<Value = Rank> {
        prop_oneof![(0usize..4).prop_map(Rank::Promoted), Just(Rank::Unpromoted)]
    }

    proptest! {
        #[test]
        fn permutation_is_a_stable_sort_and_sorting_again_changes_nothing(ranks in prop::collection::vec(rank(), 0..12)) {
            let order = permutation(&ranks);
            let mut seen = order.clone();
            seen.sort_unstable();
            prop_assert_eq!(seen, (0..ranks.len()).collect::<Vec<_>>());
            for pair in order.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                prop_assert!(ranks[a] < ranks[b] || (ranks[a] == ranks[b] && a < b));
            }
            let sorted: Vec<Rank> = order.iter().map(|&i| ranks[i]).collect();
            prop_assert_eq!(permutation(&sorted), (0..ranks.len()).collect::<Vec<_>>());
            // Whatever moved, something rose; an identity has nothing risen.
            let identity = order.iter().enumerate().all(|(to, &from)| to == from);
            prop_assert_eq!(risen(&order).is_empty(), identity);
        }
    }
}
