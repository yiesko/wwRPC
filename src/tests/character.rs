use crate::character::{all, find, normalize};

#[test]
fn normalize_meets_spaced_dashed_and_cased_input() {
    assert_eq!(normalize("Xiangli Yao"), "xiangliyao");
    assert_eq!(normalize("  CARLOTTA "), "carlotta");
    assert_eq!(normalize("rover-aero_female"), "roveraerofemale");
    assert_eq!(normalize("Firstlight's Herald"), "firstlightsherald");
    assert_eq!(normalize(""), "");
}

#[test]
fn find_resolves_known_characters() {
    let cartethyia = find("Cartethyia").unwrap();
    assert_eq!(cartethyia.asset, "cartethyia");
    assert_eq!(cartethyia.display, "Cartethyia");
    // Case, spacing and suffix variants meet the same entry.
    assert_eq!(find("cartethyia"), Some(cartethyia));
    assert_eq!(find("  CARTETHYIA "), Some(cartethyia));
    let yao = find("xiangli yao").unwrap();
    assert_eq!(yao.asset, "xiangliyao");
    assert_eq!(yao.display, "Xiangli Yao");
    let rover = find("Rover Spectro Male").unwrap();
    assert_eq!(rover.asset, "roverspectromale");
    assert_eq!(find("denia").unwrap().display, "Denia");
}

#[test]
fn find_rejects_blank_and_unknown() {
    assert_eq!(find(""), None);
    assert_eq!(find("   "), None);
    assert_eq!(find("NotACharacter"), None);
    // Legacy dash filename without a list entry: no guessing.
    assert_eq!(find("Rover-Spectro"), None);
    // Skins and UI icons are not characters.
    assert_eq!(find("YangyangXuanling"), None);
    assert_eq!(find("atk"), None);
}

#[test]
fn all_lists_every_character_once_alphabetically() {
    let listed: Vec<_> = all().collect();
    assert_eq!(listed.len(), 63);
    let mut keys: Vec<_> = listed.iter().map(|c| c.key).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), listed.len());
    assert_eq!(listed.first().unwrap().key, "aalto");
    assert_eq!(listed.last().unwrap().key, "zhezhi");
    // Assets are lowercase upload names; displays are human text.
    assert!(listed.iter().all(|c| c.asset == c.key));
    assert!(listed.iter().all(|c| !c.display.is_empty()));
}

#[test]
fn portrait_url_uses_exact_upstream_filename() {
    use crate::character::{find, portrait_url};

    // Case-sensitive CDN paths: the URL must carry the real file stem,
    // not the lowercase match key (e.g. XiangliYao, Roverelectrofemale).
    assert_eq!(
        portrait_url(&find("denia").unwrap()),
        "https://cdn.jsdelivr.net/gh/ryanbenson/wuthering-waves-assets@master/images/Denia.png"
    );
    assert_eq!(
        portrait_url(&find("xiangli yao").unwrap()),
        "https://cdn.jsdelivr.net/gh/ryanbenson/wuthering-waves-assets@master/images/XiangliYao.png"
    );
    assert_eq!(
        portrait_url(&find("Rover Electro Female").unwrap()),
        "https://cdn.jsdelivr.net/gh/ryanbenson/wuthering-waves-assets@master/images/Roverelectrofemale.png"
    );
}
