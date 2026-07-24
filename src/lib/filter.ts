import type { ImportedRecordsQuery, PersonPage, PersonQuery, PersonSummary } from "../domain/types";

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
    hotelRegionFilters: [query.hotelProvince, query.hotelCity, query.hotelCounty].map((value) =>
      splitFilterTerms(value ?? ""),
    ),
    includedHouseholdFilters: [query.householdProvince, query.householdCity, query.householdCounty].map((value) =>
      splitFilterTerms(value ?? ""),
    ),
    excludedHouseholdFilters: [
      query.excludeHouseholdProvince,
      query.excludeHouseholdCity,
      query.excludeHouseholdCounty,
    ].map((value) => splitFilterTerms(value ?? "")),
  };
}

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
    return searchable.includes(filters.searchKeyword);
  };
}

export function filterPeople(people: PersonSummary[], query: PersonQuery): PersonPage {
  const predicate = buildPersonPredicate(query);
  const filtered = people.filter(predicate);
  const start = (query.page - 1) * query.pageSize;
  return {
    items: filtered.slice(start, start + query.pageSize),
    total: filtered.length,
    page: query.page,
    pageSize: query.pageSize,
  };
}

export function splitFilterTerms(value: string): string[] {
  return [...new Set(
    value
      .split(/[,，、;；\r\n]+/)
      .map(normalize)
      .filter(Boolean),
  )];
}

function matchesEveryHotel(person: PersonSummary, keywords: string[]): boolean {
  if (keywords.length === 0) return true;
  const hotels = (person.hotelNames ?? []).map(normalize);
  return keywords.every((keyword) => hotels.some((hotel) => fuzzyIncludes(hotel, keyword)));
}

function matchesHotelRegion(person: PersonSummary, filters: string[][]): boolean {
  if (filters.every((terms) => terms.length === 0)) return true;
  return (person.hotelRegions ?? []).some((hotelRegion) => {
    const fields = [hotelRegion.province, hotelRegion.city, hotelRegion.county];
    return filters.every((terms, index) => matchesAnySubstring(fields[index] ?? "", terms));
  });
}

function matchesHouseholdRegion(
  householdFields: Array<string | undefined>,
  included: string[][],
  excluded: string[][],
): boolean {
  if (!included.every((terms, index) => matchesAnySubstring(householdFields[index] ?? "", terms))) {
    return false;
  }
  return !excluded.some((terms, index) => terms.length > 0 && matchesAnySubstring(householdFields[index] ?? "", terms));
}

function matchesAnySubstring(value: string, terms: string[]): boolean {
  if (terms.length === 0) return true;
  const normalized = normalize(value);
  return terms.some((term) => normalized.includes(term));
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
  householdProvince: string;
  householdCity: string;
  householdCounty: string;
  age: number | null;
  gender: string;
}

function buildRecordPredicate(query: ImportedRecordsQuery): (record: ImportedRecordFilterFields) => boolean {
  const filters = prepareFilters(query);
  return (record) => {
    if (filters.hotelKeywords.length > 0) {
      const hotelName = normalize(record.hotelName);
      if (!filters.hotelKeywords.every((keyword) => fuzzyIncludes(hotelName, keyword))) return false;
    }
    if (filters.hotelRegionFilters.some((terms) => terms.length > 0)) {
      const fields = [record.hotelProvince, record.hotelCity, record.hotelCounty];
      if (!filters.hotelRegionFilters.every((terms, index) => matchesAnySubstring(fields[index] ?? "", terms))) {
        return false;
      }
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
    return searchable.includes(filters.searchKeyword);
  };
}

export function recordMatchesImportedFilter(
  record: ImportedRecordFilterFields,
  query: ImportedRecordsQuery,
): boolean {
  return buildRecordPredicate(query)(record);
}
