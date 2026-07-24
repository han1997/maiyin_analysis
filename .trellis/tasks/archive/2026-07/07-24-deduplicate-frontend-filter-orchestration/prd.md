# 去重前端 filter 编排

## Goal

`src/lib/filter.ts` 中 `filterPeople` 与 `recordMatchesImportedFilter` 各自重新推导旅馆关键词、旅馆区域 splits、户籍包含/排除 splits、age/gender、search，并按几乎一致的顺序串成谓词。共享原语已复用（`splitFilterTerms`、`matchesAnySubstring`、`matchesHouseholdRegion`），但上层编排是拷贝粘贴。抽取 `prepareFilters` + `buildPersonPredicate` + `buildRecordPredicate` 中心化编排，两个导出函数只做调用与分页。**行为不变。**

## What I already know

### 当前结构

- `filterPeople`（3-52）：从 query 推导 5 组派生值（keyword、hotelKeywords、hotelRegionFilters、includedHouseholdFilters、excludedHouseholdFilters），filter 谓词含 level/alertState（person-only）+ 共享检查 + search。
- `recordMatchesImportedFilter`（129-176）：从 query 推导完全相同的 5 组派生值，谓词含共享检查 + search，无 level/alertState。
- 共享原语：`splitFilterTerms`、`normalize`、`fuzzyIncludes`、`matchesAnySubstring`、`matchesHouseholdRegion`、`matchesEveryHotel`、`matchesHotelRegion`、`splitHotelKeywords`。
- 两者均仅服务浏览器演示模式（生产走 Rust，`browserApi.ts:185` 和 `197` 调用）。
- 23 个前端测试中 filter 相关用例守卫行为（`filter.test.ts`）。

### 重复点

1. **query 派生值计算**（5 组）：两处完全一致。
2. **age/gender 检查**：两处完全一致（`minAge != null && (age == null || age < minAge)` 等）。
3. **household 检查**：两处都调 `matchesHouseholdRegion`，传参结构一致。
4. **search 检查**：模式一致（build searchable → includes），但 searchable 字段不同（person 含 level/alertTitles；record 含 hotelName/hotelRegion）。

### 不重复点（保持各自内联）

- **hotel keywords 匹配**：person 用 `person.hotelNames`（数组，每元素 fuzzyIncludes）；record 用 `record.hotelName`（单个字符串，直接 fuzzyIncludes）。
- **hotel region 匹配**：person 用 `person.hotelRegions`（数组，some + every）；record 用 `record.hotelProvince/City/County`（单组字段，直接 every）。
- **level/alertState**：仅 `filterPeople` 有。
- **searchable 字段**：person vs record 各自不同。

### 类型关系

`PersonQuery` 和 `ImportedRecordsQuery` 共享 filter 字段（search、hotelSearch、hotelProvince/City/County、householdProvince/City/County、excludeHousehold*、minAge/maxAge、gender）。TypeScript 结构类型，无需显式 `extends`。

## Decisions (locked)

- **方案**：抽取 `prepareFilters` + `buildPersonPredicate` + `buildRecordPredicate`。两个导出函数只做调用与分页。
- **`prepareFilters`**：接收结构兼容的 query，返回 `PreparedFilters`（5 组派生值）。消除重复的 query 派生值计算。
- **`buildPersonPredicate`**：返回 `(person: PersonSummary) => boolean`，内部调 `prepareFilters`，含 level/alertState + 共享检查 + search。
- **`buildRecordPredicate`**：返回 `(record: ImportedRecordFilterFields) => boolean`，内部调 `prepareFilters`，含共享检查 + search。
- **hotel keywords / hotel region / searchable 字段**：因结构不同（数组 vs 单值），保持各自 predicate 内联，不强行统一。
- **行为不变**：两个导出函数的返回值与改动前完全一致。23 个前端测试守卫。
- **质量门**：`npm run lint`、`npm run test`（23 passed）、`npm run build` 全绿。

## Requirements

### 新增 `FilterQuery` 接口

```typescript
interface FilterQuery {
  search: string;
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
  gender?: "" | "男" | "女";
}
```

PersonQuery 和 ImportedRecordsQuery 结构兼容，无需 `extends`。

### 新增 `PreparedFilters` 接口 + `prepareFilters` 函数

```typescript
interface PreparedFilters {
  searchKeyword: string;
  hotelKeywords: string[];
  hotelRegionFilters: string[][];
  includedHouseholdFilters: string[][];
  excludedHouseholdFilters: string[][];
}

function prepareFilters(query: FilterQuery): PreparedFilters {
  return {
    searchKeyword: query.search.trim().toLocaleLowerCase("zh-CN"),
    hotelKeywords: splitFilterTerms(query.hotelSearch ?? ""),
    hotelRegionFilters: [query.hotelProvince, query.hotelCity, query.hotelCounty].map((v) => splitFilterTerms(v ?? "")),
    includedHouseholdFilters: [query.householdProvince, query.householdCity, query.householdCounty].map((v) => splitFilterTerms(v ?? "")),
    excludedHouseholdFilters: [query.excludeHouseholdProvince, query.excludeHouseholdCity, query.excludeHouseholdCounty].map((v) => splitFilterTerms(v ?? "")),
  };
}
```

### 新增 `buildPersonPredicate`

```typescript
function buildPersonPredicate(query: PersonQuery): (person: PersonSummary) => boolean {
  const filters = prepareFilters(query);
  return (person) => {
    if (query.level !== "全部等级" && person.level !== query.level) return false;
    if (query.alertState === "仅预警人员" && person.alertCount === 0) return false;
    if (query.alertState === "未预警人员" && person.alertCount > 0) return false;
    if (!matchesEveryHotel(person, filters.hotelKeywords)) return false;
    if (!matchesHotelRegion(person, filters.hotelRegionFilters)) return false;
    if (!matchesHouseholdRegion(
      [person.householdProvince, person.householdCity, person.householdCounty],
      filters.includedHouseholdFilters,
      filters.excludedHouseholdFilters,
    )) return false;
    if (query.minAge != null && (person.age == null || person.age < query.minAge)) return false;
    if (query.maxAge != null && (person.age == null || person.age > query.maxAge)) return false;
    if (query.gender && person.gender !== query.gender) return false;
    if (!filters.searchKeyword) return true;
    const searchable = [person.name, person.idNo, person.phone, person.householdRegion, person.age?.toString() ?? "", person.gender, person.level, ...person.alertTitles].join(" ").toLocaleLowerCase("zh-CN");
    return searchable.includes(filters.searchKeyword);
  };
}
```

### 新增 `buildRecordPredicate`

```typescript
function buildRecordPredicate(query: ImportedRecordsQuery): (record: ImportedRecordFilterFields) => boolean {
  const filters = prepareFilters(query);
  return (record) => {
    if (filters.hotelKeywords.length > 0) {
      const hotelName = normalize(record.hotelName);
      if (!filters.hotelKeywords.every((keyword) => fuzzyIncludes(hotelName, keyword))) return false;
    }
    if (filters.hotelRegionFilters.some((terms) => terms.length > 0)) {
      const fields = [record.hotelProvince, record.hotelCity, record.hotelCounty];
      if (!filters.hotelRegionFilters.every((terms, index) => matchesAnySubstring(fields[index] ?? "", terms))) return false;
    }
    if (!matchesHouseholdRegion(
      [record.householdProvince, record.householdCity, record.householdCounty],
      filters.includedHouseholdFilters,
      filters.excludedHouseholdFilters,
    )) return false;
    if (query.minAge != null && (record.age == null || record.age < query.minAge)) return false;
    if (query.maxAge != null && (record.age == null || record.age > query.maxAge)) return false;
    if (query.gender && record.gender !== query.gender) return false;
    if (!filters.searchKeyword) return true;
    const searchable = [record.name, record.idNo, record.phone, record.hotelName, record.hotelRegion, record.householdRegion, String(record.age ?? ""), record.gender].join(" ").toLocaleLowerCase("zh-CN");
    return searchable.includes(filters.searchKeyword);
  };
}
```

### 重构 `filterPeople` 和 `recordMatchesImportedFilter`

```typescript
export function filterPeople(people: PersonSummary[], query: PersonQuery): PersonPage {
  const predicate = buildPersonPredicate(query);
  const filtered = people.filter(predicate);
  const start = (query.page - 1) * query.pageSize;
  return { items: filtered.slice(start, start + query.pageSize), total: filtered.length, page: query.page, pageSize: query.pageSize };
}

export function recordMatchesImportedFilter(record: ImportedRecordFilterFields, query: ImportedRecordsQuery): boolean {
  return buildRecordPredicate(query)(record);
}
```

## Acceptance Criteria

- [ ] `prepareFilters` 抽取完成，消除重复的 query 派生值计算。
- [ ] `buildPersonPredicate` 抽取完成，`filterPeople` 调用它。
- [ ] `buildRecordPredicate` 抽取完成，`recordMatchesImportedFilter` 调用它。
- [ ] `FilterQuery` 接口定义完成。
- [ ] `npm run lint` 零告警。
- [ ] `npm run test` 通过数仍为 23。
- [ ] `npm run build` 通过。
- [ ] `git diff` 仅 `src/lib/filter.ts` 有改动。

## Definition of Done

- 上述 AC 全部满足。
- 两个导出函数的返回值与改动前完全一致。

## Out of Scope

- 强行统一 hotel keywords / hotel region 的数组 vs 单值匹配结构。
- 改变任何筛选语义（per-field OR / cross-field AND / household exclude / search 等）。
- 新增测试。
- 改动 `browserApi.ts` 或其他调用方。

## Technical Notes

- 审计报告 P2 #8 原文：抽取 `buildRecordPredicate(query)` / `buildPersonPredicate(query)` 中心化编排，两个导出函数只做调用与分页。
- `FilterQuery` 是纯结构类型，不需要 `PersonQuery extends FilterQuery` 声明——TypeScript 结构兼容自动生效。
- `prepareFilters` 内的 `searchKeyword` 使用 `query.search.trim().toLocaleLowerCase("zh-CN")`，与两处原始代码一致。
- `buildPersonPredicate` 和 `buildRecordPredicate` 返回闭包，`prepareFilters` 在闭包创建时调用一次，而非每次 filter 回调都调——这与原代码在函数入口计算一次的模式一致。
