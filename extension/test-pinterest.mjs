import "./pinterest.js";

const px = globalThis.__inspirexPinterest;

function fail(msg) {
  console.error("FAIL:", msg);
  process.exit(1);
}

if (!px || typeof px.isPinterestUrl !== "function") {
  fail("未加载 pinterest.js API");
}

if (!px.isPinterestUrl("https://www.pinterest.com/qiubin2021/board/f1/")) {
  fail("看板 URL 应识别为 Pinterest");
}
if (!px.isPinterestUrl("https://pin.it/abc")) fail("pin.it 应识别");
if (px.isPinterestUrl("https://example.com/gallery")) fail("普通站不应识别");

const thumb = "https://i.pinimg.com/236x/ab/cd/ef/abcdef.jpg";
const orig = px.upgradePinimg(thumb);
if (!orig.includes("/originals/ab/cd/ef/")) fail("缩略图应升到 originals: " + orig);

const cands = px.pinimgCandidates(thumb);
if (!cands[0].includes("/originals/")) fail("候选首张应为 originals");
if (!cands.some((u) => u.includes("/736x/"))) fail("候选应含 736x 回退");
if (px.isJunkPinimg(thumb)) fail("236x 素材图不应当 junk");
if (!px.isJunkPinimg("https://i.pinimg.com/75x75_RS/8c/0f/38/8c0f387f0590aba804bb605fb2512251.jpg")) {
  fail("头像 75x75_RS 应过滤");
}

const data = {
  props: {
    pins: [
      {
        id: "123456789012345678",
        images: {
          orig: { url: "https://i.pinimg.com/originals/11/22/33/a.jpg" },
          "236x": { url: "https://i.pinimg.com/236x/11/22/33/a.jpg" },
        },
      },
      {
        id: "not-a-pin",
        url: "https://example.com/x.png",
      },
      {
        pinId: "999",
        images: { "736x": { url: "https://i.pinimg.com/736x/aa/bb/cc/b.jpg" } },
      },
    ],
  },
};
const pins = px.extractPinsFromData(data);
if (pins.length !== 2) fail("应从 PWS 数据抽出 2 个 Pin: " + JSON.stringify(pins));
const first = pins.find((p) => p.pinId === "123456789012345678");
if (!first || !first.pinUrl.includes("/pin/123456789012345678/")) {
  fail("Pin 出处 URL: " + JSON.stringify(first));
}
if (!first.imageUrl.includes("/originals/")) fail("PWS orig 应保留 originals");
const second = pins.find((p) => p.pinId === "999");
if (!second || !second.imageUrl.includes("/originals/")) {
  fail("736x 应升 originals: " + JSON.stringify(second));
}

console.log("OK: pinterest collect", { pins: pins.length, orig });
