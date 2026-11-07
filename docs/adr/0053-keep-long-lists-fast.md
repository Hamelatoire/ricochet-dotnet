---
status: proposed
---

# Keep long lists fast by drawing only the rows on screen

Several lists in the app grow without limit: the History, the bulk-run log, the sidebar for a large Collection, and the response tree. Each row is drawn whether or not it's visible, so a long list slows the window and the scrolling, and the app's speed is one of its main reasons to exist. We will draw only the rows that are on screen, plus a small margin, in each of those lists, and measure the cost before and after on a long list.

## Decisions within this

- **Virtualize the long lists first:** History, the bulk-run log and the sidebar tree, because they have the most rows.
- **Measure before and after:** each change records how long it takes to open and scroll a list of a fixed large size, so a regression shows.
- **Keep the same look and behaviour:** selection, keyboard movement and drag-to-reorder work the same whether a row is drawn or not.
- **Set the test size in the repo:** a fixed large sample, such as 10,000 rows, is generated from a script so that results can be repeated.

## Considered Options

- **Limit each list to the newest rows:** simple and fast, but it hides older history a person may want.
- **Paginate with buttons:** works, but it makes scrolling a long list a chore.
- **Wait until a list is slow in practice:** cheaper now, but the cost is only found when someone with a large Collection hits it.

## Consequences

- Virtualised lists are harder to test, because a row exists only while it is drawn, so the App tests in [ADR 0044](0044-test-view-models-and-headless-views.md) must cover the row logic apart from the drawing.
- Keyboard focus can be lost when a row scrolls out of view, so focus must be kept on the selected item.
- The sample data for measuring must be generated, never taken from a person's real History, in line with the rule to test with throwaway data.
