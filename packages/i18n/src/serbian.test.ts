import assert from "node:assert/strict";
import { test } from "node:test";

import { placeholdersOf } from "./check.ts";
import { serbianLatin, toSerbianLatin } from "./serbian.ts";
import { createTranslator } from "./translator.ts";

test("every letter of the alphabet, both cases", () => {
  assert.equal(
    toSerbianLatin("а б в г д ђ е ж з и ј к л љ м н њ о п р с т ћ у ф х ц ч џ ш"),
    "a b v g d đ e ž z i j k l lj m n nj o p r s t ć u f h c č dž š",
  );
  assert.equal(
    toSerbianLatin("А Б В Г Д Ђ Е Ж З И Ј К Л М Н О П Р С Т Ћ У Ф Х Ц Ч Ш"),
    "A B V G D Đ E Ž Z I J K L M N O P R S T Ć U F H C Č Š",
  );
});

test("words", () => {
  assert.equal(toSerbianLatin("Откажи"), "Otkaži");
  assert.equal(toSerbianLatin("Претрага производа"), "Pretraga proizvoda");
  assert.equal(toSerbianLatin("ђак, чаша, ћуп, шума, жаба"), "đak, čaša, ćup, šuma, žaba");
});

test("a capital digraph starts a capitalised word as Lj, Nj, Dž", () => {
  assert.equal(toSerbianLatin("Љубав Његош Џем"), "Ljubav Njegoš Džem");
});

test("a capital digraph inside a word in capitals is LJ, NJ, DŽ", () => {
  assert.equal(toSerbianLatin("ЉУБАВ ЊЕГОШ ЏЕМ"), "LJUBAV NJEGOŠ DŽEM");
  assert.equal(toSerbianLatin("ПОЉЕ КОЊ"), "POLJE KONJ");
});

test("a capital digraph alone is capitalised, not doubled", () => {
  assert.equal(toSerbianLatin("Љ"), "Lj");
  assert.equal(toSerbianLatin("Њ. Џ!"), "Nj. Dž!");
});

test("everything that isn't Serbian Cyrillic stays", () => {
  assert.equal(toSerbianLatin("Cancel 42 <b>{name}</b> ñ 日本"), "Cancel 42 <b>{name}</b> ñ 日本");
  assert.equal(toSerbianLatin(""), "");
});

test("an ICU plural keeps its syntax and formats", () => {
  const cyrillic =
    "{count, plural, =0 {Нема ставки} one {# ставка за {name}} few {# ставке} other {# ставки}}";
  const latin = toSerbianLatin(cyrillic);
  assert.equal(
    latin,
    "{count, plural, =0 {Nema stavki} one {# stavka za {name}} few {# stavke} other {# stavki}}",
  );
  assert.deepEqual(placeholdersOf(latin), placeholdersOf(cyrillic));
  const t = createTranslator("sr-Latn", { items: latin });
  assert.equal(t("items", { count: 0, name: "Ana" }), "Nema stavki");
  assert.equal(t("items", { count: 21, name: "Ana" }), "21 stavka za Ana");
  assert.equal(t("items", { count: 3, name: "Ana" }), "3 stavke");
  assert.equal(t("items", { count: 5, name: "Ana" }), "5 stavki");
});

test("tags and their contents", () => {
  assert.equal(
    toSerbianLatin("Прихватам <link>услове коришћења</link>"),
    "Prihvatam <link>uslove korišćenja</link>",
  );
});

test("serbianLatin transliterates a catalog, keeping its keys", () => {
  assert.deepEqual(serbianLatin({ close: "Затвори", next: "Следеће", page: "Страна {page}" }), {
    close: "Zatvori",
    next: "Sledeće",
    page: "Strana {page}",
  });
});
