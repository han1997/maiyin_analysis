# State Management

## Scenario: backend-paginated people results

### 1. Scope / Trigger

This contract applies whenever the result query, history loading, pagination,
filter controls, loading state, or `WorkspaceSnapshot` changes.

### 2. Signatures

```ts
interface AppApi {
  queryPeople(query: PersonQuery): Promise<PersonPage>;
}

interface PersonPage {
  items: PersonSummary[];
  total: number;
  page: number;
  pageSize: number;
}

interface ImportedRecordsPage {
  items: ImportedStayRecord[];
  total: number;
  page: number;
  pageSize: number;
}
```

### 3. Contracts

- `snapshot` is lightweight server metadata and never owns the people collection.
- `filterDraft` holds edits that are not yet applied. `query` is the applied backend
  request and resets to page `1` after snapshot actions or filter application.
- A snapshot or query change requests exactly one page through `AppApi.queryPeople`.
- Ignore late responses after effect cleanup so a slower old query cannot replace a
  newer page.
- Clear old page items when a request starts, expose `aria-busy`, show a table-shaped
  skeleton, and disable pagination until the response finishes.
- Browser mode uses the same API contract but applies `filterPeople` only to fixture
  data. Production React code never filters the full Tauri result collection locally.
- The imported-record tab owns a separate page number, page DTO, and local loading state.
  Entering the tab or changing its page requests exactly one `ImportedRecordsPage`.
- Snapshot-changing actions reset imported records to page `1`; late responses are ignored
  after effect cleanup just like people-page requests.
- Session deletion is a snapshot-changing action: keep the `delete` busy state until the
  async Tauri command resolves, disable competing session actions, and replace the shell
  with the returned snapshot (empty when the final listed session was removed).
- People and imported-record page-size controls offer exactly `50`, `100`, and `200`,
  default to `50`, and reset only their own page number to `1`. Result-filter application
  and clearing preserve the chosen people page size.
- Secondary filter and export disclosures are controlled React state, are mutually
  exclusive, and close on outside pointer interaction or `Escape`. `Escape` restores
  focus to the disclosure trigger.

### 4. Validation & Error Matrix

| Condition | UI behavior |
| --- | --- |
| Minimum age exceeds maximum age | Keep the applied query and page unchanged; show an error toast |
| Page request fails | Stop the skeleton, keep the shell usable, and show the structured error message |
| Snapshot becomes empty | Reset page state through the snapshot action and do not issue `queryPeople` |
| User changes page while a request is active | Pagination buttons remain disabled |
| An older request resolves after cleanup | Ignore its result |
| Imported-record page request fails | Stop only the records skeleton, keep both view tabs usable, and show the structured error toast |
| Page size changes | Reset that table to page `1`, clear old rows during loading, and request the chosen size |
| Outside pointer or `Escape` while a toolbar disclosure is open | Close it; do not discard filter drafts |

### 5. Good / Base / Bad Cases

- Good: loading history renders metadata immediately, then fills the first 50 rows.
- Good: applying `A，B` changes `query`, shows the local skeleton, and receives a page
  whose total was computed by SQLite.
- Good: switching to imported records leaves the shell usable while a 50-row page loads;
  switching sessions clears old rows and requests page `1`.
- Good: the filter popover escapes the result card's clipping boundary, constrains its
  height to the viewport, and scrolls internally so every field remains reachable.
- Base: browser preview waits for its fixture adapter and renders the same table shape.
- Bad: deriving `page` with `filterPeople(snapshot.people, query)` in `App.tsx`.
- Bad: leaving old-session rows visible while the next session page is loading.
- Bad: uncontrolled native `details` disclosures that stay open after outside clicks or
  allow filter and export popovers to overlap.

### 6. Tests Required

- Browser workspace waits for the asynchronous page before asserting a person row.
- Multi-hotel application returns the matching fixture person and excludes nonmatches.
- Invalid age ranges do not replace the applied page.
- Loading controls expose a stable table and accessible busy status.
- View tabs expose `tablist`, `tab`, `aria-selected`, and linked `tabpanel` semantics;
  imported-record next-page interaction renders the next fixture page.
- Page-size tests assert both tables expose `50/100/200`, reset to page `1`, and keep the
  selection through result-filter changes.
- Disclosure tests assert mutual exclusion, outside-pointer close, `Escape` close, and
  accessible `aria-expanded` state.
- `npm test`, `npm run lint`, and `npm run build` pass after contract changes.

### 7. Wrong vs Correct

#### Wrong

```ts
const page = filterPeople(snapshot.people, query);
```

#### Correct

```ts
const page = await appApi.queryPeople(query);
setQuery((current) => ({ ...current, page: 1, pageSize: 100 }));
```

The correct path keeps the WebView memory and IPC payload proportional to page size.
