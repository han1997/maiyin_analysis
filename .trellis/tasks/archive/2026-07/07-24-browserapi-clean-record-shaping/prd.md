# 清理 browserApi toImportedStayRecord 的字段裁剪

## Goal

`browserApi.ts` 的 `toImportedStayRecord`（82-105）用解构 + 9 个 `void` 丢弃 `ImportedRecordFilterFields` 独有字段，意图不直观。改为显式重建 `ImportedStayRecord` 对象，只保留需要的字段。**行为不变。**

## What I already know

### 当前代码

```typescript
function toImportedStayRecord(record: DemoImportedRecord): ImportedStayRecord {
  const {
    hotelProvince, hotelCity, hotelCounty, hotelRegion,
    householdProvince, householdCity, householdCounty,
    age, gender,
    ...rest
  } = record;
  void hotelProvince; void hotelCity; void hotelCounty; void hotelRegion;
  void householdProvince; void householdCity; void householdCounty;
  void age; void gender;
  return rest;
}
```

### 类型关系

- `DemoImportedRecord extends ImportedStayRecord, ImportedRecordFilterFields`（line 39）。
- `ImportedStayRecord`（14 字段）：uid, sourceFile, sourceRow, name, idNo, phone, householdRegion, hotelName, region, address, roomNo, checkIn, registerTime, checkOut, issues。
- `ImportedRecordFilterFields`（14 字段）：name, idNo, phone, hotelName, hotelProvince, hotelCity, hotelCounty, hotelRegion, householdRegion, householdProvince, householdCity, householdCounty, age, gender。
- 共享字段：name, idNo, phone, hotelName, householdRegion。
- `ImportedRecordFilterFields` 独有（需丢弃）：hotelProvince, hotelCity, hotelCounty, hotelRegion, householdProvince, householdCity, householdCounty, age, gender（9 个）。
- `ImportedStayRecord` 独有（需保留）：uid, sourceFile, sourceRow, region, address, roomNo, checkIn, registerTime, checkOut, issues（10 个）。
- 当前 `...rest` 包含共享 + `ImportedStayRecord` 独有 = 15 个字段。

### 调用点

`browserApi.ts:202`：`items: filtered.slice(start, end).map(toImportedStayRecord)`。仅用于将 demo 数据从 `DemoImportedRecord` 裁剪为 `ImportedStayRecord` 返回给 UI。

## Decisions (locked)

- **方案**：显式重建 `ImportedStayRecord` 对象，逐字段从 `record` 取值。不引入 `omit` 工具——只需改一处，工具函数反而增加间接性。
- **行为不变**：返回的对象字段与改动前完全一致。
- **质量门**：`npm run lint`、`npm run test`（23 passed）、`npm run build` 全绿。

## Requirements

将 `toImportedStayRecord` 改为：

```typescript
function toImportedStayRecord(record: DemoImportedRecord): ImportedStayRecord {
  return {
    uid: record.uid,
    sourceFile: record.sourceFile,
    sourceRow: record.sourceRow,
    name: record.name,
    idNo: record.idNo,
    phone: record.phone,
    householdRegion: record.householdRegion,
    hotelName: record.hotelName,
    region: record.region,
    address: record.address,
    roomNo: record.roomNo,
    checkIn: record.checkIn,
    registerTime: record.registerTime,
    checkOut: record.checkOut,
    issues: record.issues,
  };
}
```

## Acceptance Criteria

- [ ] `toImportedStayRecord` 不再使用解构 + `void` 模式。
- [ ] 返回的 `ImportedStayRecord` 字段与改动前完全一致。
- [ ] `npm run lint` 零告警。
- [ ] `npm run test` 通过数仍为 23。
- [ ] `npm run build` 通过。
- [ ] `git diff` 仅 `src/api/browserApi.ts` 有改动。

## Out of Scope

- 引入 `omit` 工具函数。
- 改变 `DemoImportedRecord` 或 `ImportedStayRecord` 的类型定义。
- 改动其他函数。

## Technical Notes

- 审计报告 P3 #14 原文：用一个小 `omit` 工具或显式重建对象更清晰。本方案选显式重建——只需改一处，工具函数反而增加间接性。
- 显式重建的字段列表对应 `ImportedStayRecord` 的全部 14 字段（types.ts:78-94），TypeScript 编译器会检查字段完整性。
