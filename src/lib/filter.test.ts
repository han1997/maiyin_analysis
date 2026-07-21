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
  hotelNames: ["临涧如雅民宿", "阊江商务酒店"],
  hotelRegions: [
    { province: "安徽省", city: "黄山市", county: "祁门县", region: "安徽省 黄山市 祁门县" },
    { province: "浙江省", city: "杭州市", county: "西湖区", region: "浙江省 杭州市 西湖区" },
  ],
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

  it("requires every separated hotel name to match", () => {
    for (const hotelSearch of ["临雅民宿,阊江", "临雅民宿，阊江", "临雅民宿、阊江"]) {
      expect(
        filterPeople([person], {
          search: "",
          hotelSearch,
          level: "全部等级",
          alertState: "全部人员",
          page: 1,
          pageSize: 50,
        }).total,
      ).toBe(1);
    }
    expect(
      filterPeople([person], {
        search: "",
        hotelSearch: "临雅民宿,牯牛降",
        level: "全部等级",
        alertState: "全部人员",
        page: 1,
        pageSize: 50,
      }).total,
    ).toBe(0);
  });

  it("filters hotel region, household region and person attributes after analysis", () => {
    expect(
      filterPeople([person], {
        search: "",
        hotelProvince: "安徽",
        hotelCity: "黄山",
        hotelCounty: "祁门",
        householdProvince: "安徽",
        householdCounty: "祁门",
        excludeHouseholdCounty: "休宁",
        minAge: 30,
        maxAge: 40,
        gender: "男",
        level: "全部等级",
        alertState: "全部人员",
        page: 1,
        pageSize: 50,
      }).total,
    ).toBe(1);

    expect(
      filterPeople([person], {
        search: "",
        hotelProvince: "安徽",
        hotelCounty: "西湖",
        level: "全部等级",
        alertState: "全部人员",
        page: 1,
        pageSize: 50,
      }).total,
    ).toBe(0);
  });

  it("excludes unknown ages when an age boundary is active", () => {
    expect(
      filterPeople([{ ...person, age: null }], {
        search: "",
        minAge: 18,
        level: "全部等级",
        alertState: "全部人员",
        page: 1,
        pageSize: 50,
      }).total,
    ).toBe(0);
  });
});
