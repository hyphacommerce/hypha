-- Tax classes and regions (the hypha-tax crate, design note 0003). Constraint names are
-- part of the adapter contract: error.rs maps them to `hypha_tax::repo::keys`.
 
CREATE TABLE tax_classes (
    id      uuid   CONSTRAINT tax_classes_pkey PRIMARY KEY,
    version bigint NOT NULL CHECK (version > 0),
    name    text   NOT NULL
);
 
-- A product is in at most one class: the product is the key. No foreign key to products:
-- extensions and other domains link to them by ID only (design note 0002).
CREATE TABLE product_tax_classes (
    product_id   uuid PRIMARY KEY,
    tax_class_id uuid NOT NULL REFERENCES tax_classes (id) ON DELETE CASCADE
);
 
CREATE INDEX product_tax_classes_class ON product_tax_classes (tax_class_id);
 
-- Rates are parts per million: 0 ..= 1000000 (100%).
CREATE TABLE tax_regions (
    id           uuid    CONSTRAINT tax_regions_pkey PRIMARY KEY,
    version      bigint  NOT NULL CHECK (version > 0),
    country      text    NOT NULL CONSTRAINT tax_region_country UNIQUE,
    default_rate integer NOT NULL CHECK (default_rate BETWEEN 0 AND 1000000)
);
 
-- No foreign key to tax_classes: a separate aggregate, checked by set_class_rate.
CREATE TABLE tax_region_class_rates (
    tax_region_id uuid    NOT NULL REFERENCES tax_regions (id) ON DELETE CASCADE,
    position      integer NOT NULL,
    tax_class_id  uuid    NOT NULL,
    rate          integer NOT NULL CHECK (rate BETWEEN 0 AND 1000000),
    PRIMARY KEY (tax_region_id, tax_class_id),
    UNIQUE (tax_region_id, position)
);