import { describe, expect, it } from "vitest";
import type { PersonSummary } from "../domain/types";
import { filterPeople } from "./filter";

const person: PersonSummary = {
  personKey: "1",
  name: "周明远",
  idNo: "341024198809128135",
  phone: "13905591234",
  householdRegion: "安徽省 黄山市 祁门县",
  age: 37,
  gender: "男",
  totalRecords: 9,
  maxMonthCount: 8,
  maxYearCount: 9,
  overlapDays: 1,
  sequentialDays: 0,
  score: 77,
  level: "中风险",
  alertCount: 2,
  alertTitles: ["不同住宿地点时间重合", "30 天高频入住"],
  hotelNames: ["临涧如雅民宿"],
};

describe("filterPeople", () => {
  it("searches identity, household region and alert text", () => {
    for (const search of ["341024", "祁门县", "时间重合"]) {
      const page = filterPeople([person], {
        search,
        level: "全部等级",
        alertState: "全部人员",
        page: 1,
        pageSize: 50,
      });
      expect(page.total).toBe(1);
    }
  });

  it("supports risk and alert filters", () => {
    expect(
      filterPeople([person], {
        search: "",
        level: "高风险",
        alertState: "全部人员",
        page: 1,
        pageSize: 50,
      }).total,
    ).toBe(0);
    expect(
      filterPeople([person], {
        search: "",
        level: "全部等级",
        alertState: "仅预警人员",
        page: 1,
        pageSize: 50,
      }).total,
    ).toBe(1);
  });

  it("supports fuzzy hotel-name matching", () => {
    expect(
      filterPeople([person], {
        search: "",
        hotelSearch: "临雅民宿",
        level: "全部等级",
        alertState: "全部人员",
        page: 1,
        pageSize: 50,
      }).total,
    ).toBe(1);
  });
});
