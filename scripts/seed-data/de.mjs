// The demo store's German content: what `scripts/seed.mjs` writes in English, translated,
// keyed by the seed's own stable names (handles, field keys, choice keys, facet keys,
// titles), never by generated IDs. `scripts/seed-translations.mjs` reads each resource's
// translation view and matches its fields here.
//
// The shop says "du", as German hobby shops do. One term per thing throughout: Zuchtset
// (grow kit), Myzel, Fruchtkörper, Körnerbrut (grain spawn), Impfspritze, Substrat,
// Austernseitling, Igelstachelbart (lion's mane), Laminar-Flow-Werkbank, Still-Air-Box.
// A text that reads the same in German ("10 ml", "EU", "Shiitake") isn't listed: the
// source shows.

/** The language's tag. */
export const LANGUAGE = "de";

/** The store's description. */
export const STORE = {
  description:
    "Zuchtsets, Laborkulturen und aus Myzel gewachsene Materialien, aus unserem eigenen Labor und von unserem eigenen Hof.",
};

/** Option names, by the English name: every product shares them. */
export const OPTION_NAMES = {
  Species: "Sorte",
  Size: "Größe",
  Length: "Länge",
  Shelves: "Böden",
  Humidifier: "Luftbefeuchter",
  Volume: "Volumen",
  Weight: "Gewicht",
  Pack: "Packung",
  Capacity: "Fassungsvermögen",
  Plug: "Stecker",
  Width: "Breite",
  Colour: "Farbe",
  Thickness: "Stärke",
  Shape: "Form",
  Finish: "Oberfläche",
};

/** The species, by the English name: option values, the `species` field's choices, alt texts. */
const SPECIES = {
  "Pearl oyster": "Austernseitling",
  "Blue oyster": "Blauer Austernseitling",
  "Pink oyster": "Rosa Seitling",
  "Golden oyster": "Limonenseitling",
  "King oyster": "Kräuterseitling",
  "Lion's mane": "Igelstachelbart",
  Chestnut: "Chestnut-Schüppling",
};

/** Option value labels, by the English label. */
export const OPTION_VALUES = {
  ...SPECIES,
  Family: "Familiengröße",
  "2 shelves": "2 Böden",
  "4 shelves": "4 Böden",
  None: "Ohne",
  Ultrasonic: "Ultraschall",
  "10 plates": "10 Platten",
  "25 plates": "25 Platten",
  "2.5 kg": "2,5 kg",
  Natural: "Natur",
  Charcoal: "Anthrazit",
  Forest: "Waldgrün",
  Clay: "Terrakotta",
  "1.2 mm": "1,2 mm",
  "1.8 mm": "1,8 mm",
  "Wine bottle": "Weinflasche",
  "Box insert 20 × 15 cm": "Karton-Einlage 20 × 15 cm",
  "Corner protector": "Kantenschutz",
  Raw: "Roh",
  Charred: "Geflammt",
  Dome: "Kuppel",
  Cone: "Kegel",
  "Self-paced": "Im eigenen Tempo",
  "Live cohort": "Live-Gruppe",
  Paperback: "Taschenbuch",
  Hardcover: "Gebundene Ausgabe",
  eBook: "E-Book",
};

/** Custom field values several products share, by field key, then by the English value. */
export const VALUES = {
  "custom.grown_in": {
    "Our lab and farm": "Unser Labor und Hof",
    "Our forest plots": "Unsere Waldflächen",
  },
};

/** Products by handle: title, description (HTML) and their own custom field values. */
export const PRODUCTS = {
  "oyster-grow-kit": {
    title: "Austernseitling-Zuchtset",
    description:
      "<p>Ein Block aus Hartholz und Sojaschalen, bereits vollständig von Myzel durchwachsen. Schneide das Fenster auf, besprühe es zweimal am Tag und ernte nach etwa zehn Tagen dein erstes Büschel, zwei Wochen später gleich noch einmal.</p>",
    fields: {
      "custom.care_guide":
        "<ol><li>Schneide ein X in das Fenster des Beutels.</li><li>Besprühe den Schnitt zweimal am Tag und halte ihn aus der direkten Sonne.</li><li>Ernte, sobald sich die Hüte abflachen, und dreh das ganze Büschel auf einmal ab.</li><li>Lass den Block zwei Wochen ruhen und weiche ihn über Nacht ein: Dann bildet er erneut Fruchtkörper.</li></ol>",
    },
  },
  "lions-mane-grow-kit": {
    title: "Igelstachelbart-Zuchtset",
    description:
      "<p>Der Igelstachelbart wächst als weiße Kaskade aus weichen Stacheln und schmeckt in Butter gebraten wie Krabbenfleisch. Langsamer als ein Austernseitling, aber das Warten lohnt sich.</p>",
  },
  "shiitake-log": {
    title: "Shiitake-Holzstamm",
    description:
      "<p>Ein Eichenstamm, angebohrt und mit Shiitake-Impfdübeln bestückt. Stell ihn schattig in den Garten: Er bildet jedes Frühjahr und jeden Herbst Fruchtkörper, vier bis sechs Jahre lang.</p>",
  },
  "reishi-antler-kit": {
    title: "Reishi-Geweih-Zuchtset",
    description:
      "<p>Ohne frische Luft bildet Reishi statt Hüten lackrote Geweihe. Zieh sie für Tee oder trockne sie als Skulptur.</p>",
  },
  "fruiting-chamber": {
    title: "Fruchtungskammer",
    description:
      "<p>Ein Zelt mit Regalböden, das die Luftfeuchtigkeit um deine Blöcke hält. Die Ultraschall-Version steuert einen Vernebler über einen Feuchtigkeitssensor, sodass du auch übers Wochenende wegfahren kannst.</p>",
  },
  "liquid-culture-syringe": {
    title: "Flüssigkultur-Impfspritze",
    description:
      "<p>Lebendes Myzel in einer Nährlösung, gezogen aus unseren Mutterkulturen und vor dem Versand auf Agar auf Kontamination geprüft. Impfe damit Körner oder einen sterilisierten Substratbeutel.</p>",
  },
  "grain-spawn-bag": {
    title: "Körnerbrut-Beutel",
    description:
      "<p>Sterilisierter Roggen, vollständig vom Myzel der gewählten Sorte durchwachsen, mit Filterpatch, damit er atmen kann. Misch ihn unter ein Massensubstrat und starte deine eigene Zucht.</p>",
  },
  "agar-plates": {
    title: "Agarplatten",
    description:
      "<p>Gegossener und versiegelter Malzextrakt-Agar in 90-mm-Schalen: um aus einer Gewebeprobe eine saubere Kultur zu isolieren oder eine Impfspritze zu prüfen, bevor du ihr vertraust.</p>",
  },
  "hardwood-substrate": {
    title: "Angereichertes Hartholzsubstrat",
    description:
      "<p>Eichensägemehl mit 20 % Sojaschalen, befeuchtet und im Filterbeutel sterilisiert. Bereit für Körnerbrut: der Master’s Mix für Austernseitling, Igelstachelbart und Shiitake.</p>",
  },
  "coffee-substrate": {
    title: "Kaffeesatz-Substrat",
    description:
      "<p>Kaffeesatz, am Morgen des Brühens bei zwölf Cafés aus der Nachbarschaft abgeholt und mit Stroh pasteurisiert. Austernseitlinge lieben ihn, und er erspart jede Woche einen Container voll Abfall.</p>",
  },
  "pressure-sterilizer": {
    title: "Drucksterilisator",
    description:
      "<p>Ein Sterilisator aus Aluminiumguss, der 121 °C bei 1 bar Überdruck hält: die Temperatur, die jede Spore abtötet, die sich in einem Körnerglas verstecken kann. Zwei Manometer, ein Gewichtsventil und eine Dichtung, an die du nie wieder denken musst.</p>",
  },
  "flow-hood": {
    title: "Laminar-Flow-Werkbank",
    description:
      "<p>Eine Wand aus gefilterter Luft, die mit 0,45 m/s über deine Arbeitsfläche strömt, sodass nichts, was im Raum schwebt, eine offene Platte erreicht. Die Antwort der Profis auf Kontamination.</p>",
  },
  "still-air-box": {
    title: "Still-Air-Box",
    description:
      "<p>Eine durchsichtige Box mit zwei Armöffnungen. Stehende Luft trägt keine Sporen: In einer ausgewischten Box gelingen dir mit ruhigen Händen saubere Überimpfungen, ganz ohne Laminar-Flow-Werkbank.</p>",
  },
  "inoculation-kit": {
    title: "Impfbesteck-Set",
    description:
      "<p>Ein Skalpell mit zehn Klingen, eine Impföse, eine Spirituslampe, sterile Kanülen und eine Rolle Parafilm, in einer Segeltuchrolle, die in eine Still-Air-Box passt.</p>",
  },
  "mycelium-leather": {
    title: "Myzel-Leder",
    description:
      "<p>Ein geschmeidiges Material, aus Reishi-Myzel auf Hanf gewachsen, mit Pflanzenextrakten gegerbt und als laufender Meter in 140 cm Breite verkauft. Es lässt sich schneiden und nähen wie Leder und altert auch so.</p>",
    fields: {
      "custom.care_guide":
        "<p>Mit einem feuchten Tuch abwischen und zweimal im Jahr mit einem Pflanzenwachs pflegen. Vor langer, direkter Sonne schützen: Sie bleicht den Naturton zuerst aus.</p>",
    },
  },
  "packaging-inserts": {
    title: "Gewachsene Verpackungseinlagen",
    description:
      "<p>Schutzeinlagen, in fünf Tagen in Formen aus Hanf und Myzel gewachsen. So leicht und stabil wie Styropor, und im Gartenkompost zersetzen sie sich in etwa sechs Wochen.</p>",
  },
  "acoustic-panel": {
    title: "Akustik-Wandpaneel",
    description:
      "<p>Auf Hanf gewachsene Paneele mit tiefem, unregelmäßigem Relief, das Schall streut und schluckt. Montiert werden sie mit der verdeckten Keilleiste; die geflammten sind abgeflämmt und bekommen so eine dunklere, härtere Oberfläche.</p>",
  },
  "pendant-shade": {
    title: "Gewachsener Lampenschirm",
    description:
      "<p>Ein Lampenschirm, eine Woche lang in einer Form gewachsen, dann getrocknet und in der Struktur belassen, die das Myzel gewählt hat. Keiner gleicht dem anderen. Passt auf eine E27-Pendelleuchte.</p>",
  },
  "cultivation-course": {
    title: "Pilzzucht: Von der Spore zur Ernte",
    description:
      "<p>Zwölf Lektionen aus unserem Labor: steriles Arbeiten, Agar, Flüssigkultur, Körnerbrut und Fruchtung, mit den Fehlern, die wir gemacht haben, damit du sie nicht machst. Im eigenen Tempo oder in einer Gruppe mit vier Live-Sessions.</p>",
  },
  "mycelium-handbook": {
    title: "Das Myzel-Handbuch",
    description:
      "<p>Unser Laborhandbuch, für alle neu geschrieben: 320 Seiten über das Züchten, Kochen und Bauen mit Pilzen, mit Fotos von unserem Hof.</p>",
  },
  "mycelium-brick": {
    title: "Myzel-Baustein",
    description:
      "<p>Ein tragfähiger Baustein, aus Hanf und Myzel gewachsen. Derzeit im Drucktest an der Fakultät für Bauingenieurwesen.</p>",
  },
};

/** Categories by handle: name, description (HTML) and the buying guide (`custom.buying_guide`). */
export const CATEGORIES = {
  grow: {
    name: "Zucht",
    description: "<p>Pilzzucht von Mycelium Works.</p>",
    guide:
      "<p>Neu in der Pilzzucht? Fang mit einem <strong>Austernseitling-Zuchtset</strong> an: Es bildet nach etwa zehn Tagen auf der Küchenarbeitsplatte Fruchtkörper. Igelstachelbart und Shiitake brauchen mehr Geduld; Reishi ist etwas für alle, die ein Projekt suchen.</p>",
  },
  "grow-kits": { name: "Zuchtsets" },
  cultures: { name: "Kulturen & Brut" },
  substrates: { name: "Substrate" },
  lab: {
    name: "Labor",
    description: "<p>Laborausrüstung von Mycelium Works.</p>",
    guide:
      "<p>Sauberes Arbeiten ist der größte Teil der Mykologie. Mit einer <strong>Still-Air-Box</strong> kommen Einsteiger weit; eine <strong>Laminar-Flow-Werkbank</strong> und ein <strong>Drucksterilisator</strong> machen aus einem Hobby ein Labor.</p>",
  },
  sterilization: { name: "Sterilisation" },
  "clean-air": { name: "Reine Luft" },
  tools: { name: "Werkzeuge" },
  materials: {
    name: "Materialien",
    description: "<p>Materialien von Mycelium Works.</p>",
    guide:
      "<p>Myzel wächst in jede Form, die man ihm gibt, und wird dann getrocknet, damit es aufhört zu wachsen. Das Ergebnis ersetzt Leder, Schaumstoff und Kunststoff und kehrt am Ende in den Boden zurück.</p>",
  },
  leather: { name: "Myzel-Leder" },
  packaging: { name: "Verpackung" },
  acoustics: { name: "Akustikpaneele" },
  objects: { name: "Objekte" },
  learn: {
    name: "Lernen",
    description: "<p>Kurse und Bücher von Mycelium Works.</p>",
    guide:
      "<p>Vom ersten Zuchtset bis zu eigenen Kulturen: Kurse und Bücher von unserem Laborteam.</p>",
  },
  courses: { name: "Kurse" },
  books: { name: "Bücher" },
};

/**
 * Custom field definitions by owner type, then by key (namespace `custom`): name,
 * description and choice labels by choice key. Only the fields storefronts read: staff-only
 * ones are read in the admin, which shows the source.
 */
export const FIELDS = {
  product: {
    difficulty: {
      name: "Schwierigkeit",
      description: "Wie viel Erfahrung es voraussetzt.",
      choices: { beginner: "Einsteiger", intermediate: "Fortgeschritten", advanced: "Profi" },
    },
    live_culture: {
      name: "Lebende Kultur",
      description:
        "Lebt beim Versand: Es geht nur montags bis mittwochs auf die Reise, damit es nie ein Wochenende in einem Depot liegt.",
    },
    first_harvest_days: { name: "Tage bis zur ersten Ernte" },
    expected_yield: { name: "Erwarteter Ertrag pro Erntewelle" },
    substrate: {
      name: "Substrat",
      choices: {
        hardwood: "Hartholzsägemehl",
        soy_hulls: "Sojaschalen",
        straw: "Stroh",
        coffee: "Kaffeesatz",
        rye: "Roggenkörner",
        hemp: "Hanfschäben",
      },
    },
    certifications: {
      name: "Zertifizierungen",
      choices: {
        eu_organic: "EU-Bio",
        home_compostable: "Heimkompostierbar (TÜV OK compost HOME)",
        vegan: "Vegan (ohne tierische Bestandteile)",
        en_1822: "Filter nach EN 1822 geprüft",
        ce: "CE-gekennzeichnet",
      },
    },
    fire_rating: { name: "Brandverhalten (EN 13501-1)" },
    sound_absorption: {
      name: "Schallabsorption (NRC)",
      description: "Der Schallabsorptionsgrad: 0 reflektiert alles, 1 schluckt alles.",
    },
    warranty_months: { name: "Garantie (Monate)" },
    spec_sheet: { name: "Datenblatt" },
    works_with: { name: "Passt zu", description: "Produkte, für die es gemacht ist." },
    care_guide: { name: "Pflegeanleitung" },
    grown_in: { name: "Herkunft" },
    next_session: { name: "Nächste Live-Session" },
    duration_hours: { name: "Dauer (Stunden)" },
  },
  variant: {
    species: {
      name: "Sorte",
      choices: {
        pearl_oyster: SPECIES["Pearl oyster"],
        blue_oyster: SPECIES["Blue oyster"],
        pink_oyster: SPECIES["Pink oyster"],
        golden_oyster: SPECIES["Golden oyster"],
        king_oyster: SPECIES["King oyster"],
        lions_mane: SPECIES["Lion's mane"],
        chestnut: SPECIES.Chestnut,
      },
    },
    volume: { name: "Volumen" },
    generation: {
      name: "Kulturgeneration",
      description: "Überimpfungen seit der Mutterkultur: je niedriger, desto vitaler.",
    },
    lab_tested_on: { name: "Auf Kontamination geprüft" },
    capacity: { name: "Fassungsvermögen" },
    voltage: { name: "Spannung" },
    thickness: { name: "Stärke" },
    width: { name: "Breite" },
    height: { name: "Höhe" },
  },
  category: {
    buying_guide: { name: "Kaufberatung" },
  },
};

/** Search filter labels, by facet key. */
export const FACETS = {
  "field.custom.species": "Sorte",
  "field.custom.difficulty": "Schwierigkeit",
  "field.custom.live_culture": "Lebende Kultur",
  "field.custom.certifications": "Zertifizierungen",
  product_type: "Produktart",
  "option.colour": "Farbe",
  vendor: "Hersteller",
  on_sale: "Im Angebot",
};

/** Shipping option names, by the English name. Carriers' own names stay as they are. */
export const SHIPPING_OPTIONS = {
  Standard: "Standardversand",
  Express: "Expressversand",
  Ground: "Bodenversand",
  "2-Day": "Zustellung in 2 Tagen",
};

/** Payment methods, by the English name. */
export const PAYMENT_METHODS = {
  "Cash on delivery": {
    name: "Nachnahme",
    instructions: "Bezahle den Kurier bar, wenn deine Bestellung ankommt.",
  },
  "Bank transfer": {
    name: "Banküberweisung",
    instructions: [
      "Überweise den Gesamtbetrag an:",
      "Mycelium Works Ltd., Demo Bank",
      "IBAN: XX00 DEMO 0000 0000 0000 00",
      "BIC: DEMOXX2X",
      "Gib deine Bestellnummer als Verwendungszweck an. Wir versenden, sobald die Zahlung eingegangen ist.",
    ].join("\n"),
  },
};

/** Discount titles, by the English title. */
export const DISCOUNTS = {
  "Welcome offer": "Willkommensangebot",
  "Free shipping over €50": "Kostenloser Versand ab 50 €",
  "Free shipping over £45": "Kostenloser Versand ab 45 £",
  "Free shipping over $60": "Kostenloser Versand ab 60 $",
  "Free shipping over 6,000 RSD": "Kostenloser Versand ab 6.000 RSD",
  "Buy a grow kit, get a syringe free": "Kaufe ein Zuchtset, erhalte eine Impfspritze gratis",
  "Culture volume discount": "Mengenrabatt auf Kulturen",
};

/** Checkout rules' messages, by the rule's name. */
export const CHECKOUT_RULES = {
  "No PO boxes for express":
    "Expresskuriere können nicht an Postfächer liefern. Wähle den Standardversand oder gib eine Straßenadresse an.",
  "House number required": "Gib deine Hausnummer in der ersten Adresszeile an.",
};
