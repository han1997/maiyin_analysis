# 合并 activeExtraFilterCount 与 activeRecordsFilterCount

## Goal

`appHelpers.ts` 中 `activeExtraFilterCount` 与 `activeRecordsFilterCount` 前 5 项计数完全一致，仅末尾"预警状态"一项不同。抽 `activeSharedFilterCount` 共享前 5 项，两个导出函数只做调用 + 差异项。**行为不变。**

## What I already know

### 当前代码

```typescript
export function activeExtraFilterCount(query: PersonQuery): number {
  return Number(splitFilterTerms(query.hotelSearch ?? "").length > 0)
    + activeDelimitedFieldCount([query.hotelProvince, query.hotelCity, query.hotelCounty])
    + activeDelimitedFieldCount([query.householdProvince, query.householdCity, query.householdCounty])
    + activeDelimitedFieldCount([query.excludeHouseholdProvince, query.excludeHouseholdCity, query.excludeHouseholdCounty])
    + Number(query.minAge != null || query.maxAge != null || Boolean(query.gender))
    + Number((query.alertState ?? "全部人员") !== "全部人员");
}

export function activeRecordsFilterCount(query: ImportedRecordsQuery): number {
  return Number(splitFilterTerms(query.hotelSearch ?? "").length > 0)
    + activeDelimitedFieldCount([query.hotelProvince, query.hotelCity, query.hotelCounty])
    + activeDelimitedFieldCount([query.householdProvince, query.householdCity, query.householdCounty])
    + activeDelimitedFieldCount([query.excludeHouseholdProvince, query.excludeHouseholdCity, query.excludeHouseholdCounty])
    + Number(query.minAge != null || query.maxAge != null || Boolean(query.gender));
}
```

### 差异

- 前 5 项（hotelSearch + 3 组 activeDelimitedFieldCount + age/gender）完全一致。
- `activeExtraFilterCount` 多第 6 项：`Number((query.alertState ?? "全部人员") !== "全部人员")`。
- `activeRecordsFilterCount` 无第 6 项（`ImportedRecordsQuery` 无 `alertState`）。

### 调用点

- `App.tsx:30-31`：import 两个函数。
- `App.tsx:582`：`activeRecordsFilterCount(recordsFilterDraft)`。
- `App.tsx:624`：`activeExtraFilterCount(filterDraft)`。

## Decisions (locked)

- **方案**：抽 `activeSharedFilterCount(query)` 共享前 5 项。`activeExtraFilterCount` 调它 + alertState 项；`activeRecordsFilterCount` 直接调它。
- **类型**：`activeSharedFilterCount` 接收一个内部 `FilterCountQuery` 接口（结构兼容 PersonQuery 和 ImportedRecordsQuery），无需导入 filter.ts 的 `FilterQuery`。
- **行为不变**：两个导出函数返回值与改动前完全一致。
- **质量门**：`npm run lint`、`npm run test`（23 passed）、`npm run build` 全绿。

## Requirements

### 新增 `FilterCountQuery` 接口 + `activeSharedFilterCount` 函数

```typescript
interface FilterCountQuery {
  hotelSearch?: string;
  hotelProvince?: string;
  hotelCity?: string;
  hotelCounty?: string;
  householdProvince?: string;
  householdCity?: string;
  householdCounty?: string;
  excludeHouseholdProvince?: string;
  excludeHouseholdCity?: string;
  excludeHouseholdCounty?: string;
  minAge?: number | null;
  maxAge?: number | null;
  gender?: string;
}

function activeSharedFilterCount(query: FilterCountQuery): number {
  return Number(splitFilterTerms(query.hotelSearch ?? "").length > 0)
    + activeDelimitedFieldCount([query.hotelProvince, query.hotelCity, query.hotelCounty])
    + activeDelimitedFieldCount([query.householdProvince, query.householdCity, query.householdCounty])
    + activeDelimitedFieldCount([query.excludeHouseholdProvince, query.excludeHouseholdCity, query.excludeHouseholdCounty])
    + Number(query.minAge != null || query.maxAge != null || Boolean(query.gender));
}
```

### 重构两个导出函数

```typescript
export function activeExtraFilterCount(query: PersonQuery): number {
  return activeSharedFilterCount(query)
    + Number((query.alertState ?? "全部人员") !== "全部人员");
}

export function activeRecordsFilterCount(query: ImportedRecordsQuery): number {
  return activeSharedFilterCount(query);
}
```

## Acceptance Criteria

- [ ] `activeSharedFilterCount` 抽取完成。
- [ ] `activeExtraFilterCount` 调用 `activeSharedFilterCount` + alertState 项。
- [ ] `activeRecordsFilterCount` 调用 `activeSharedFilterCount`。
- [ ] `npm run lint` 零告警。
- [ ] `npm run test` 通过数仍为 23。
- [ ] `npm run build` 通过。
- [ ] `git diff` 仅 `src/lib/appHelpers.ts` 有改动。

## Out of Scope

- 改变计数逻辑或字段。
- 新增测试。
- 改动 `App.tsx` 或其他调用方。

## Technical Notes

- 审计报告 P3 #15 原文：抽 `activeFilterCount(query, includeAlertState)` 共享前 4 段。本方案用 `activeSharedFilterCount` + 各自差异项，更清晰。
- `FilterCountQuery` 是 appHelpers.ts 内部接口，不导出。
