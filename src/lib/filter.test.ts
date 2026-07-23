import { describe, expect, it } from "vitest";
import type { PersonSummary } from "../domain/types";
import {
  filterPeople,
  recordMatchesImportedFilter,
  splitFilterTerms,
  type ImportedRecordFilterFields,
} from "./filter";

const person: PersonSummary = {
  personKey: "1",
  name: "周明远",
  idNo: "341024198809128135",
  phone: "13905591234",
  householdRegion: "安徽省 黄山市 祁门县",
  householdProvince: "安徽省",
  householdCity: "黄山市",
  householdCounty: "祁门县",
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
  it("normalizes all supported separators, whitespace, case and duplicate terms", () => {
    expect(splitFilterTerms(" 安徽,浙 江，江苏、四川;重庆；北京\n天津\r上海；安 徽 ")).toEqual([
      "安徽",
      "浙江",
      "江苏",
      "四川",
      "重庆",
      "北京",
      "天津",
      "上海",
    ]);
    expect(splitFilterTerms("An Hui, an hui")).toEqual(["anhui"]);
  });

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

  it("uses fuzzy multi-value OR within region fields and AND across fields", () => {
    expect(
      filterPeople([person], {
        search: "",
        hotelProvince: "江苏，徽 省",
        hotelCity: "南京；山 市",
        hotelCounty: "西湖\n门 县",
        householdProvince: "浙江，徽 省",
        householdCounty: "休宁、门 县",
        excludeHouseholdCounty: "西湖；休宁",
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
        excludeHouseholdProvince: "浙江；徽省",
        level: "全部等级",
        alertState: "全部人员",
        page: 1,
        pageSize: 50,
      }).total,
    ).toBe(0);

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

  it("applies the same fuzzy multi-value semantics to imported records", () => {
    const record: ImportedRecordFilterFields = {
      name: "周明远",
      idNo: "341024198809128135",
      phone: "13905591234",
      hotelName: "阊江商务酒店",
      hotelProvince: "安徽省",
      hotelCity: "黄山市",
      hotelCounty: "祁门县",
      hotelRegion: "安徽省 黄山市 祁门县",
      householdRegion: "安徽省 黄山市 祁门县",
      householdProvince: "安徽省",
      householdCity: "黄山市",
      householdCounty: "祁门县",
      age: 37,
      gender: "男",
    };

    expect(recordMatchesImportedFilter(record, {
      search: "",
      hotelProvince: "浙江,徽省",
      hotelCity: "杭州、山市",
      householdProvince: "江苏;徽省",
      householdCounty: "西湖\n门县",
      excludeHouseholdCity: "成都；上海",
      page: 1,
      pageSize: 50,
    })).toBe(true);

    expect(recordMatchesImportedFilter(record, {
      search: "",
      excludeHouseholdCounty: "西湖,门县",
      page: 1,
      pageSize: 50,
    })).toBe(false);
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
