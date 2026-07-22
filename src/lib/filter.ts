import type { ImportedRecordsQuery, PersonPage, PersonQuery, PersonSummary } from "../domain/types";

export function filterPeople(people: PersonSummary[], query: PersonQuery): PersonPage {
  const keyword = query.search.trim().toLocaleLowerCase("zh-CN");
  const hotelKeywords = splitHotelKeywords(query.hotelSearch ?? "");
  const hotelRegionFilters = [query.hotelProvince, query.hotelCity, query.hotelCounty].map((value) => normalize(value ?? ""));
  const includedHouseholdFilters = [query.householdProvince, query.householdCity, query.householdCounty]
    .map((value) => normalize(value ?? ""))
    .filter(Boolean);
  const excludedHouseholdFilters = [query.excludeHouseholdProvince, query.excludeHouseholdCity, query.excludeHouseholdCounty]
    .map((value) => normalize(value ?? ""))
    .filter(Boolean);
  const filtered = people.filter((person) => {
    if (query.level !== "全部等级" && person.level !== query.level) return false;
    if (query.alertState === "仅预警人员" && person.alertCount === 0) return false;
    if (query.alertState === "未预警人员" && person.alertCount > 0) return false;
    if (!matchesEveryHotel(person, hotelKeywords)) return false;
    if (!matchesHotelRegion(person, hotelRegionFilters)) return false;
    if (!matchesHouseholdRegion(person.householdRegion, includedHouseholdFilters, excludedHouseholdFilters)) return false;
    if (query.minAge != null && (person.age == null || person.age < query.minAge)) return false;
    if (query.maxAge != null && (person.age == null || person.age > query.maxAge)) return false;
    if (query.gender && person.gender !== query.gender) return false;
    if (!keyword) return true;

    const searchable = [
      person.name,
      person.idNo,
      person.phone,
      person.householdRegion,
      person.age?.toString() ?? "",
      person.gender,
      person.level,
      ...person.alertTitles,
    ]
      .join(" ")
      .toLocaleLowerCase("zh-CN");

    return searchable.includes(keyword);
  });

  const start = (query.page - 1) * query.pageSize;
  return {
    items: filtered.slice(start, start + query.pageSize),
    total: filtered.length,
    page: query.page,
    pageSize: query.pageSize,
  };
}

function splitHotelKeywords(value: string): string[] {
  return value
    .split(/[,，、;；\n]+/)
    .map(normalize)
    .filter(Boolean);
}

function matchesEveryHotel(person: PersonSummary, keywords: string[]): boolean {
  if (keywords.length === 0) return true;
  const hotels = (person.hotelNames ?? []).map(normalize);
  return keywords.every((keyword) => hotels.some((hotel) => fuzzyIncludes(hotel, keyword)));
}

function matchesHotelRegion(person: PersonSummary, filters: string[]): boolean {
  if (filters.every((value) => !value)) return true;
  return (person.hotelRegions ?? []).some((hotelRegion) => {
    const fields = [hotelRegion.province, hotelRegion.city, hotelRegion.county];
    return filters.every((filter, index) => {
      if (!filter) return true;
      return [fields[index] ?? "", hotelRegion.region].some((value) => normalize(value).includes(filter));
    });
  });
}

export function matchesHouseholdRegion(householdRegion: string, included: string[], excluded: string[]): boolean {
  const value = normalize(householdRegion);
  if (!included.every((item) => value.includes(item))) return false;
  return excluded.length === 0 || !excluded.every((item) => value.includes(item));
}

export function normalize(value: string): string {
  return value.trim().toLocaleLowerCase("zh-CN").replace(/\s+/g, "");
}

export function fuzzyIncludes(value: string, query: string): boolean {
  if (value.includes(query)) return true;
  let index = 0;
  for (const character of value) {
    if (character === query[index]) index += 1;
    if (index === query.length) return true;
  }
  return false;
}

export interface ImportedRecordFilterFields {
  name: string;
  idNo: string;
  phone: string;
  hotelName: string;
  hotelProvince: string;
  hotelCity: string;
  hotelCounty: string;
  hotelRegion: string;
  householdRegion: string;
  age: number | null;
  gender: string;
}

export function recordMatchesImportedFilter(
  record: ImportedRecordFilterFields,
  query: ImportedRecordsQuery,
): boolean {
  const hotelKeywords = splitHotelKeywords(query.hotelSearch ?? "");
  if (hotelKeywords.length > 0) {
    const hotelName = normalize(record.hotelName);
    if (!hotelKeywords.every((keyword) => fuzzyIncludes(hotelName, keyword))) return false;
  }
  const hotelRegionFilters = [query.hotelProvince, query.hotelCity, query.hotelCounty].map((value) =>
    normalize(value ?? ""),
  );
  if (hotelRegionFilters.some((value) => value)) {
    const fields = [record.hotelProvince, record.hotelCity, record.hotelCounty];
    if (!hotelRegionFilters.every((filter, index) => !filter || normalize(fields[index] ?? "").includes(filter))) {
      return false;
    }
  }
  const includedHouseholdFilters = [query.householdProvince, query.householdCity, query.householdCounty]
    .map((value) => normalize(value ?? ""))
    .filter(Boolean);
  const excludedHouseholdFilters = [query.excludeHouseholdProvince, query.excludeHouseholdCity, query.excludeHouseholdCounty]
    .map((value) => normalize(value ?? ""))
    .filter(Boolean);
  if (!matchesHouseholdRegion(record.householdRegion, includedHouseholdFilters, excludedHouseholdFilters)) {
    return false;
  }
  if (query.minAge != null && (record.age == null || record.age < query.minAge)) return false;
  if (query.maxAge != null && (record.age == null || record.age > query.maxAge)) return false;
  if (query.gender && record.gender !== query.gender) return false;
  const search = query.search.trim().toLocaleLowerCase("zh-CN");
  if (!search) return true;
  const searchable = [
    record.name,
    record.idNo,
    record.phone,
    record.hotelName,
    record.hotelRegion,
    record.householdRegion,
    String(record.age ?? ""),
    record.gender,
  ]
    .join(" ")
    .toLocaleLowerCase("zh-CN");
  return searchable.includes(search);
}
