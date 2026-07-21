import type { PersonPage, PersonQuery, PersonSummary } from "../domain/types";

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

function matchesHouseholdRegion(householdRegion: string, included: string[], excluded: string[]): boolean {
  const value = normalize(householdRegion);
  if (!included.every((item) => value.includes(item))) return false;
  return excluded.length === 0 || !excluded.every((item) => value.includes(item));
}

function normalize(value: string): string {
  return value.trim().toLocaleLowerCase("zh-CN").replace(/\s+/g, "");
}

function fuzzyIncludes(value: string, query: string): boolean {
  if (value.includes(query)) return true;
  let index = 0;
  for (const character of value) {
    if (character === query[index]) index += 1;
    if (index === query.length) return true;
  }
  return false;
}
