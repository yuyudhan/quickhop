//! Property tests for the launcher view model: whatever the input sequence, the selection stays
//! valid, the root page is never lost, and enough `Esc` presses always hide the window.

use proptest::prelude::*;
use quickhop_core::{Action, Item, ItemId, Ranked, Tier};
use quickhop_ui::model::{Effect, Model, Page, PageKind};

/// One user or window input.
#[derive(Debug, Clone)]
enum Input {
    Type(String),
    /// Results for the search `age` searches before the newest one (0: the newest).
    Results {
        age: u64,
        count: usize,
    },
    Next,
    Previous,
    Click(usize),
    Enter,
    Actions,
    Escape,
    PushList,
    PushDetail,
}

fn input() -> impl Strategy<Value = Input> {
    prop_oneof![
        "[a-c ]{0,4}".prop_map(Input::Type),
        (0u64..3, 0usize..6).prop_map(|(age, count)| Input::Results { age, count }),
        Just(Input::Next),
        Just(Input::Previous),
        (0usize..8).prop_map(Input::Click),
        Just(Input::Enter),
        Just(Input::Actions),
        Just(Input::Escape),
        Just(Input::PushList),
        Just(Input::PushDetail),
    ]
}

fn results(count: usize) -> Vec<Ranked> {
    (0..count)
        .map(|n| {
            let mut item = Item::new(
                ItemId::new("p", n.to_string()),
                format!("item {n}"),
                Action::Copy {
                    text: n.to_string(),
                },
            );
            // Zero, one or two actions, so empty action panels are exercised too.
            item.actions.truncate(n % 2);
            if n % 3 == 2 {
                item.actions.push(Action::Open {
                    target: format!("https://example.com/{n}"),
                });
            }
            Ranked {
                item,
                tier: Tier::Matched,
                score: 0,
                positions: Vec::new(),
            }
        })
        .collect()
}

/// Applies `input`, tracking the newest search number the model asked for.
fn apply(model: &mut Model, newest: &mut u64, input: &Input) -> Effect {
    let effect = match input {
        Input::Type(text) => model.set_query(text.clone()),
        Input::Results { age, count } => {
            model.set_results(newest.saturating_sub(*age), results(*count));
            Effect::None
        }
        Input::Next => {
            model.select_next();
            Effect::None
        }
        Input::Previous => {
            model.select_previous();
            Effect::None
        }
        Input::Click(index) => {
            model.select(*index);
            Effect::None
        }
        Input::Enter => model.activate(),
        Input::Actions => {
            model.toggle_actions();
            Effect::None
        }
        Input::Escape => model.escape(),
        Input::PushList => model.push(Page::new(
            "list",
            PageKind::List {
                command: ItemId::new("p", "list"),
            },
        )),
        Input::PushDetail => model.push(Page::new(
            "detail",
            PageKind::Detail {
                text: "text".into(),
            },
        )),
    };
    if let Effect::Search(request) = &effect {
        assert!(
            request.generation > *newest,
            "a new search must have a larger number than every earlier one"
        );
        *newest = request.generation;
    }
    effect
}

proptest! {
    #[test]
    fn selections_stay_valid(inputs in prop::collection::vec(input(), 0..60)) {
        let mut model = Model::new();
        let mut newest = 0;
        for input in &inputs {
            apply(&mut model, &mut newest, input);
            let page = model.page();
            prop_assert_eq!(page.selected().is_none(), page.results().is_empty());
            prop_assert!(page.selected().is_none_or(|s| s < page.results().len()));
            if let Some(panel) = model.panel() {
                prop_assert_eq!(panel.selected().is_none(), panel.entries().is_empty());
                prop_assert!(panel.selected().is_none_or(|s| s < panel.entries().len()));
                prop_assert!(panel.entries().iter().all(|e| e.action < panel.item().actions.len()));
            }
            prop_assert!(!model.stack().is_empty());
            prop_assert_eq!(&model.stack()[0].kind, &PageKind::Root);
        }
    }

    #[test]
    fn performed_actions_belong_to_their_item(inputs in prop::collection::vec(input(), 0..60)) {
        let mut model = Model::new();
        let mut newest = 0;
        for input in &inputs {
            if let Effect::Perform { item, action, .. } = apply(&mut model, &mut newest, input) {
                prop_assert!(item.actions.contains(&action));
            }
        }
    }

    #[test]
    fn enough_escapes_always_hide(inputs in prop::collection::vec(input(), 0..60)) {
        let mut model = Model::new();
        let mut newest = 0;
        for input in &inputs {
            apply(&mut model, &mut newest, input);
        }
        // At most one Esc for the panel and one per pushed page, then one more hides.
        let presses = model.stack().len() + 1;
        let hid = (0..presses).any(|_| model.escape() == Effect::Hide);
        prop_assert!(hid);
        prop_assert_eq!(model.stack().len(), 1);
    }
}
