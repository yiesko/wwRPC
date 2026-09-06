// wwrpc - Wuthering Waves Discord Rich Presence for Linux.
// Copyright (C) 2026 yiesko
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version. See COPYING for details.

//! Playable-character icon keys for `--character`.
//!
//! Portrait source of truth (images only, never vendored here — the
//! upstream repo carries no license and the art belongs to Kuro Games):
//! `ryanbenson/wuthering-waves-assets`, `images/*.png`, PascalCase stems
//! (`Carlotta.png`, `XiangliYao.png`, Rover variants with a `Male`/`Female`
//! suffix). Discord only renders assets uploaded to the application, so a
//! name here is just a KEY: upload the PNG to the app under the same
//! lowercase name, then `--character <name>`.
//!
//! New characters are one line in the `CHARACTERS` table below (plus the
//! portal upload).

/// One selectable character: match key, Discord asset key, hover text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Character {
    /// Normalized match key (lowercase alphanumeric, e.g. `xiangliyao`).
    pub key: &'static str,
    /// Discord art-asset key: file stem lowercased (e.g. `cartethyia`).
    pub asset: &'static str,
    /// Hover text (e.g. `Xiangli Yao`).
    pub display: &'static str,
}

/// `(key, asset, display)`, alphabetical by key. Generated from the
/// upstream `images/` listing; Rover variants keep their suffix.
const CHARACTERS: &[(&str, &str, &str)] = &[
    ("aalto", "aalto", "Aalto"),
    ("aemeath", "aemeath", "Aemeath"),
    ("augusta", "augusta", "Augusta"),
    ("baizhi", "baizhi", "Baizhi"),
    ("brant", "brant", "Brant"),
    ("buling", "buling", "Buling"),
    ("calcharo", "calcharo", "Calcharo"),
    ("camellya", "camellya", "Camellya"),
    ("cantarella", "cantarella", "Cantarella"),
    ("carlotta", "carlotta", "Carlotta"),
    ("cartethyia", "cartethyia", "Cartethyia"),
    ("changli", "changli", "Changli"),
    ("chisa", "chisa", "Chisa"),
    ("chixia", "chixia", "Chixia"),
    ("ciaccona", "ciaccona", "Ciaccona"),
    ("danjin", "danjin", "Danjin"),
    ("denia", "denia", "Denia"),
    ("encore", "encore", "Encore"),
    ("galbrena", "galbrena", "Galbrena"),
    ("hiyuki", "hiyuki", "Hiyuki"),
    ("iuno", "iuno", "Iuno"),
    ("jianxin", "jianxin", "Jianxin"),
    ("jingran", "jingran", "Jingran"),
    ("jinhsi", "jinhsi", "Jinhsi"),
    ("jiyan", "jiyan", "Jiyan"),
    ("lingyang", "lingyang", "Lingyang"),
    ("lucilla", "lucilla", "Lucilla"),
    ("lucy", "lucy", "Lucy"),
    ("lumi", "lumi", "Lumi"),
    ("lupa", "lupa", "Lupa"),
    ("luukherssen", "luukherssen", "LuukHerssen"),
    ("lynae", "lynae", "Lynae"),
    ("mornye", "mornye", "Mornye"),
    ("mortefi", "mortefi", "Mortefi"),
    ("phoebe", "phoebe", "Phoebe"),
    ("phrolova", "phrolova", "Phrolova"),
    ("qingxiao", "qingxiao", "Qingxiao"),
    ("qiuyuan", "qiuyuan", "Qiuyuan"),
    ("rebecca", "rebecca", "Rebecca"),
    ("roccia", "roccia", "Roccia"),
    ("roveraerofemale", "roveraerofemale", "Rover (Aero, Female)"),
    ("roveraeromale", "roveraeromale", "Rover (Aero, Male)"),
    (
        "roverelectrofemale",
        "roverelectrofemale",
        "Rover (Electro, Female)",
    ),
    (
        "roverelectromale",
        "roverelectromale",
        "Rover (Electro, Male)",
    ),
    (
        "roverhavocfemale",
        "roverhavocfemale",
        "Rover (Havoc, Female)",
    ),
    ("roverhavocmale", "roverhavocmale", "Rover (Havoc, Male)"),
    (
        "roverspectrofemale",
        "roverspectrofemale",
        "Rover (Spectro, Female)",
    ),
    (
        "roverspectromale",
        "roverspectromale",
        "Rover (Spectro, Male)",
    ),
    ("sanhua", "sanhua", "Sanhua"),
    ("shorekeeper", "shorekeeper", "Shorekeeper"),
    ("sigrika", "sigrika", "Sigrika"),
    ("suisui", "suisui", "Suisui"),
    ("taoqi", "taoqi", "Taoqi"),
    ("verina", "verina", "Verina"),
    ("xiangliyao", "xiangliyao", "Xiangli Yao"),
    ("yangyang", "yangyang", "Yangyang"),
    ("yinlin", "yinlin", "Yinlin"),
    ("youhu", "youhu", "Youhu"),
    ("yuanwu", "yuanwu", "Yuanwu"),
    ("zani", "zani", "Zani"),
    ("zhezhi", "zhezhi", "Zhezhi"),
];

/// Normalize free input for matching: lowercase alphanumeric only, so
/// `Xiangli Yao`, `xiangli-yao` and `XiangliYao` all meet `xiangliyao`.
pub fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Resolve `--character` input to a [`Character`]. `None` for blank or
/// unknown input — the caller warns and publishes without an icon.
pub fn find(input: &str) -> Option<Character> {
    let key = normalize(input);
    if key.is_empty() {
        return None;
    }
    CHARACTERS
        .iter()
        .find(|(known, _, _)| *known == key)
        .map(|(key, asset, display)| Character {
            key,
            asset,
            display,
        })
}

/// Every selectable character, alphabetical. Backs `--list-characters`.
pub fn all() -> impl Iterator<Item = Character> {
    CHARACTERS.iter().map(|(key, asset, display)| Character {
        key,
        asset,
        display,
    })
}
