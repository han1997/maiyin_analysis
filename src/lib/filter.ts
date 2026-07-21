import type { PersonPage, PersonQuery, PersonSummary } from "../domain/types";

export function filterPeople(people: PersonSummary[], query: PersonQuery): PersonPage {
  const keyword = query.search.trim().toLocaleLowerCase("zh-CN");
  const hotelKeyword = normalize(query.hotelSearch ?? "");
  const filtered = people.filter((person) => {
    if (query.level !== "全部等级" && person.level !== query.level) return false;
    if (query.alertState === "仅预警人员" && person.alertCount === 0) return false;
    if (query.alertState === "未预警人员" && person.alertCount > 0) return false;
    if (hotelKeyword && !(person.hotelNames ?? []).some((hotel) => fuzzyIncludes(normalize(hotel), hotelKeyword))) return false;
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
