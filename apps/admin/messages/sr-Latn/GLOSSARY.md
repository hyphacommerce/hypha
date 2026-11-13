# Rečnik: admin na srpskom (latinica, `sr-Latn`)

The terms this catalog uses, for whoever reviews or extends it. Ekavian, Latin script with
full diacritics. Where the storefront's Serbian catalog (`apps/storefront/messages/sr-Latn`)
names the same thing, the admin uses its word.

## Tone

- Buttons, menu items and row actions: second person singular imperative (`Sačuvaj`,
  `Otkaži`, `Dodaj proizvod`), as in the storefront.
- Sentences, hints, placeholders and errors addressed to the reader: formal `vi`, lower
  case (`Unesite iznos.`, `Vaša uloga…`).
- Dialog titles that ask: an infinitive question (`Obrisati „{name}“?`); dialog titles that
  name a task: a verbal noun (`Dodavanje adrese`, `Izmena kupca`).
- Where the reader's gender would show, both forms (`Sačuvao/la sam ih`,
  `Odobrio/la: {person}`) or a neutral rewording.
- Quotation marks are `„…“`. Decimal examples a person types use a comma where the parser
  takes both (`18,50`); examples of stored or exact formats keep the point (`12.50 EUR`,
  `4.125`, `2026-10-20`).

## Inserted names

Serbian inflects nouns, and a product title, a role's name or a field label arrives in the
nominative. So a name is never put where another case is needed:

- after a colon: `Izmeni: {name}`, `Ukloni: {name}`, `Popust {off}: {items}`,
  `{field}: prazno`;
- in apposition to a noun that carries the case: `za varijantu {variant}`,
  `iz kategorije {category}`, `aplikacija {app} nije odgovorila`;
- in quotes after a verb: `Obrisati „{name}“?`.

## Terms

| English | Srpski | Note |
| --- | --- | --- |
| store | prodavnica | |
| storefront | internet prodavnica | the shop's site; the storefront itself says `prodavnica` to shoppers |
| admin | admin | |
| order | porudžbina | storefront |
| purchase order | narudžbenica | the document sent to a supplier; never `porudžbina` |
| ordering from a supplier | narudžbina, naručiti | the act; `porudžbina` stays the shopper's |
| net 30 (payment terms) | valuta plaćanja: 30 d. | |
| cart | korpa | storefront |
| checkout | završetak kupovine | storefront |
| customer, shopper, buyer | kupac | one word for all three |
| customer group | grupa kupaca | |
| guest checkout | kupovina u svojstvu gosta | |
| product | proizvod | |
| variant | varijanta | |
| option (of a product) | opcija | |
| item (in a cart or discount) | artikal / stavka / komad | `stavka` for a line, `komad` for a counted unit |
| unit (of stock) | jedinica | |
| category / subcategory | kategorija / potkategorija | |
| vendor | brend | storefront (`Brend: {vendor}`) |
| tag (product) | tag | |
| label (customer) | etiketa | kept apart from `tag` and `oznaka` |
| handle | URL oznaka | |
| SKU | SKU | declined with a hyphen: `SKU-a`, `SKU-ovi` |
| inventory, stock | zalihe | |
| on hand / in stock | na stanju | storefront |
| reserved / available / incoming | rezervisano / dostupno / u dolasku | |
| adjust stock | korigovati zalihe | `korekcija zaliha` |
| location | lokacija | |
| warehouse | skladište | |
| fulfilment, to fulfil | isporuka, isporučiti | storefront (`isporučeno`) |
| shipping (to a shopper) | dostava | storefront |
| shipment | pošiljka | storefront |
| shipped | poslato | |
| to take back (a shipment, a receipt) | povući | |
| carrier | kurirska služba | |
| tracking number | broj za praćenje | |
| shipping zone / option / rate | zona dostave / opcija dostave / cena dostave | |
| rate app | aplikacija za cene dostave | |
| market | tržište | |
| channel, sales channel | kanal, kanal prodaje | |
| catalog | katalog | |
| price list | cenovnik | |
| base price | osnovna cena | |
| fixed price | fiksna cena | |
| compare-at price | uporedna cena | |
| adjustment (price list) | korekcija | |
| markup | uvećanje | |
| sale (price list kind) | sniženje | `sniženo` for "on sale", as in the storefront |
| clearance | rasprodaja | |
| wholesale | veleprodaja | |
| quantity breaks | cene po količini | storefront; one break is a `prag` |
| discount | popust | |
| discount code | kôd za popust | |
| batch (of codes) | serija | |
| tier | nivo | |
| buy X get Y | kupi X, dobij Y | |
| subtotal / total / tax | međuzbir / ukupno / porez | storefront |
| payment / payment method | plaćanje / način plaćanja | storefront |
| payment provider | pružalac usluge plaćanja | storefront |
| cash on delivery | plaćanje pouzećem | |
| bank transfer | uplata na račun | |
| pending / authorized / paid | na čekanju (čeka se uplata) / autorizovano / plaćeno | storefront |
| refunded / voided | refundirano / poništeno | storefront |
| cancelled | otkazano | storefront |
| purchasing | nabavka | |
| supplier | dobavljač | |
| receipt (of goods), to receive | prijem, primiti | |
| accepted / refused (units) | prihvaćeno / odbijeno | |
| unit cost (on a purchase order) | jedinična cena | the supplier's price |
| extra costs | zavisni troškovi | the accounting term (`zavisni troškovi nabavke`) |
| landed cost; an item's cost | nabavna cena | price plus its share of the extra costs |
| margin | marža | |
| exchange rate | kurs | |
| payment terms | uslovi plaćanja | |
| lead time | rok isporuke | |
| transfer | prenos | |
| in transit | u tranzitu | |
| staff | osoblje | `član osoblja` for one person |
| role | uloga | |
| scope, permission | dozvola | scope names (`orders:read`) stay as they are |
| grant (of an app or agent) | ovlašćenje | the set of scopes approved |
| owner | vlasnik | |
| invite | pozivnica | |
| to revoke | opozvati | |
| session | sesija | |
| sign in / sign out | prijaviti se / odjaviti se | |
| password | lozinka | |
| two-factor authentication | dvofaktorska autentifikacija | |
| authenticator app | aplikacija za autentifikaciju | |
| recovery code | rezervni kôd | |
| email | imejl, imejl adresa | storefront |
| app | aplikacija | |
| custom app | ručno pravljena aplikacija | |
| private app | privatna aplikacija | |
| spore | spora | |
| webhook | webhook | declined with a hyphen: `webhook-ovi` |
| signing secret | tajna za potpisivanje | |
| API key | API ključ | |
| secret / publishable key | tajni / javni ključ | |
| storefront key | ključ internet prodavnice | |
| agent | agent | |
| proposal, to propose, to apply | predlog, predložiti, primeniti | |
| preview | pregled | |
| deployment | instalacija | |
| operator | operater | |
| custom field | prilagođeno polje | |
| choice (field type) | ponuđena vrednost | |
| media / file | mediji / fajl | |
| alt text | alternativni tekst | |
| upload | otpremiti | |
| template | šablon | |
| layout (email) | izgled | |
| notification | obaveštenje | |
| address check / verification | provera adrese / verifikacija | |
| verdict | ishod (verifikacije) | |
| flag (of an address) | obeležje | |
| PO box | poštanski fah | |
| checkout rule | pravilo završetka kupovine | |
| preset | gotovo pravilo | |
| draft / active / archived | nacrt / aktivan / arhiviran | the adjective agrees with what it describes |
| scheduled / expired | zakazano / isteklo | |
| settings | podešavanja | |
| setup guide | vodič za podešavanje | |
| search index | indeks pretrage | |
| erase (personal data) | izbrisati lične podatke | `obrisati` is an ordinary delete |
| consent (marketing) | saglasnost | |
| opt-in | prijava | `jednostruka` / `dvostruka prijava` |

## Kept in English

Scope names, event types, operation IDs, header and setting names, file names, code
samples, `Hypha`, `Stripe`, `MCP`, `OAuth`, `JSON`, `CSV`, `SKU`, `URL`, `callback`,
`webhook`, `bearer token`, `manifest`.

## Plurals

Every plural has `one`, `few` and `other` (1 proizvod, 2 proizvoda, 5 proizvoda; 21 is
`one`, 22 is `few`, 11–14 are `other`). The `one` branch always carries `#`, since it
also answers 21, 31, 101. Where a count stands apart from its noun, the sentence is turned
so no agreement is needed (`Izabrano: #`, `neuspelo: #`).
