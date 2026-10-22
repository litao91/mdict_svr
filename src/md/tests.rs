//! Regression tests for the HTML → Markdown conversion.
//!
//! Every fixture is a real snippet trimmed from the dictionaries in `./data`,
//! and each test is named after the specific failure it guards — most of them
//! are things the previous `html2md`-based converter got wrong on this corpus.

use super::{decode_record, html_to_markdown};

// --- the generic core -------------------------------------------------------

#[test]
fn converts_basic_html() {
    assert_eq!(html_to_markdown("<p>hello <b>world</b></p>"), "hello **world**\n");
}

/// Source indentation is not content. OALD9 records are pretty-printed HTML, and
/// treating the leading whitespace as significant turned whole entries into
/// 4-space indented code blocks.
#[test]
fn indented_source_is_not_a_code_block() {
    // source: OALD9 "apple"
    let html = r##"<div xmlns="http://www.w3.org/1999/xhtml" class="entry" id="apple">
             <div class="h-g" id="apple__1">
              <div class="top-container">
               <div class="top-g" id="apple__2">
                <div class="webtop-g">
                 <span class="z">&nbsp;</span>
                 <h2 class="h">apple</h2>
                 <span class="z">&nbsp;</span>
                </div>
               </div>
              </div>
             </div>
            </div>"##;
    let md = html_to_markdown(html);
    assert_eq!(md, "## apple\n");
    assert!(!md.contains("\n    "), "indented code block: {md:?}");
}

#[test]
fn headings_are_atx_never_setext() {
    let md = html_to_markdown("<h2 class=\"h\">apple</h2>");
    assert_eq!(md, "## apple\n");
    assert!(!md.contains("---"));
}

/// `<br>` is the primary line separator in CALD4, COBUILD8, oxfordPrimary and
/// XinNiujin, so the two-space hard break must survive normalization.
#[test]
fn br_becomes_a_hard_break() {
    assert_eq!(html_to_markdown("<span>a<br>b</span>"), "a  \nb\n");
}

#[test]
fn leading_and_trailing_br_produce_no_blank_line() {
    // source: COBUILD8, every record opens with a bare <br>
    assert_eq!(html_to_markdown("<br><b>X</b><br>"), "**X**\n");
}

/// An inline element promoted to a block by an inline `style` is a line break in
/// the original design. CALD4 builds its whole entry out of these.
#[test]
fn style_display_block_separates_lines() {
    // source: CALD4 "apple"
    let html = r##"<span style="margin-left:20px;display:block;"><font color="gray">» to peel an apple</font></span>
<span style="margin-left:20px;display:block;"><font color="gray">» apple <b>pie/sauce</b></font></span>"##;
    assert_eq!(
        html_to_markdown(html),
        "» to peel an apple\n\n» apple **pie/sauce**\n"
    );
}

/// OxEnCh8 uses a `display` *attribute* rather than CSS.
#[test]
fn display_attribute_separates_lines() {
    // source: OxEnCh8 "apple" — <span level="3" display="block" class="block-g">
    assert_eq!(
        html_to_markdown(r##"<span display="block">a</span><span>b</span>"##),
        "a\n\nb\n"
    );
}

#[test]
fn emphasis_reopens_across_a_block_boundary() {
    assert_eq!(
        html_to_markdown("<b>foo<div>bar</div>baz</b>"),
        "**foo**\n\n**bar**\n\n**baz**\n"
    );
}

#[test]
fn empty_emphasis_leaves_no_trace() {
    assert_eq!(html_to_markdown("<p>a<b></b>c</p>"), "ac\n");
}

#[test]
fn nested_lists_indent_by_two_spaces() {
    assert_eq!(
        html_to_markdown("<ul><li>a<ul><li>b</li></ul></li></ul>"),
        "- a\n\n  - b\n"
    );
}

#[test]
fn an_li_with_no_list_ancestor_is_unordered() {
    assert_eq!(html_to_markdown("<span><li>x</li></span>"), "- x\n");
}

/// OALD9 wraps whole entries in `<ol class="h-g">` whose children are `div` and
/// `span`, never `li`. Numbering those emits a run of empty `1.` `2.` markers.
#[test]
fn ol_without_li_children_is_a_plain_block() {
    // source: OALD9 "run" — <ol class="h-g" id="run_1__1"><div class="top-container">
    let html = r##"<ol class="h-g"><div class="top-container"><span class="def">x</span></div></ol>"##;
    let md = html_to_markdown(html);
    assert_eq!(md, "x\n");
    assert!(!md.lines().any(|l| l.starts_with("1.")), "{md:?}");
}

#[test]
fn sub_and_sup_degrade_to_plain_text() {
    // `~x~` is strikethrough in GFM, so `<sub>` must not map to it.
    // source: CALD4 "apple" — <u>Thesaurus</u><sup>+</sup>
    assert_eq!(html_to_markdown("<u>Thesaurus</u><sup>+</sup>:"), "Thesaurus+:\n");
    assert_eq!(html_to_markdown("H<sub>2</sub>O"), "H2O\n");
}

// --- whitespace and invisible characters ------------------------------------

/// col pads every record with ~475 tab characters; `way` has 19,428.
#[test]
fn tab_runs_collapse_to_one_space() {
    // source: col "run"
    let html = "<p>\t\t\t<span>V-T/V-I</span>\t\t\t\tWhen you <b>run</b>\t\t</p>";
    assert_eq!(html_to_markdown(html), "V-T/V-I When you **run**\n");
}

/// OxEnCh8 separates its inflections with `<span class="smart-blank"> </span>`.
/// Dropping whitespace-only nodes outright would glue them into "gowentgoinggone".
#[test]
fn whitespace_only_nodes_still_separate_atoms() {
    // source: OxEnCh8 "go"
    let html = r##"<span class="inflection">go</span><span><span class="smart-blank"> </span></span><span class="inflection">went</span><span><span class="smart-blank"> </span></span><span class="inflection">going</span>"##;
    assert_eq!(html_to_markdown(html), "go went going\n");
}

#[test]
fn cjk_gains_no_spurious_spaces() {
    assert_eq!(html_to_markdown(r##"<span class="chn">蘋果</span>"##), "蘋果\n");
    assert_eq!(html_to_markdown("<span>蘋</span><span>果</span>"), "蘋果\n");
}

#[test]
fn invisible_characters_are_normalized() {
    // OALD9 carries 103,505 U+200B, including inside headwords.
    assert_eq!(html_to_markdown("<p>a\u{200b}b</p>"), "ab\n");
    assert_eq!(html_to_markdown("<p>a\u{00ad}b</p>"), "ab\n");
    assert_eq!(html_to_markdown("<p>a\u{feff}b</p>"), "ab\n");
    // Non-breaking and ideographic spaces are ordinary separators.
    assert_eq!(html_to_markdown("<p>a\u{00a0}b</p>"), "a b\n");
    assert_eq!(html_to_markdown("<p>a\u{3000}b</p>"), "a b\n");
    assert_eq!(html_to_markdown("<p>a\u{2009}b</p>"), "a b\n");
}

/// oxfordPrimary writes `&nbsp` with no semicolon 23,156 times.
#[test]
fn legacy_entity_without_semicolon() {
    // source: oxfordPrimary — <FONT face="Arial" color="#0" size="5">&nbsp;&nbsp;x</FONT>
    assert_eq!(html_to_markdown("<font>&nbsp a&nbsp b</font>"), "a b\n");
}

// --- noise filtering --------------------------------------------------------

#[test]
fn head_link_script_meta_dropped_wherever_they_appear() {
    // col repeats a full <head> block mid-record after an <hr/>; html5ever
    // ignores the <head> start tag in body but still inserts its children there.
    let html = r##"<p>a</p><head>
<link rel="stylesheet" type="text/css" href="result-min.css"/>
<script type="text/javascript" src="yd_col.js"></script>
<meta charset="utf-8">
</head><p>b</p>"##;
    assert_eq!(html_to_markdown(html), "a\n\nb\n");
}

#[test]
fn stray_style_end_tag_inside_a_link() {
    // oxfordPrimary has 8,206 `</style>` end tags with no opener, several of
    // them inside the headword's audio link.
    let html = r##"<a href="sound://audio/apple.mp3" title="朗读" class="t2"></style><font face="Arial" color="#2894ff" size="7"><b>apple</b></font></a>"##;
    assert_eq!(
        html_to_markdown(html),
        "[**apple**](sound://audio/apple.mp3)\n"
    );
}

#[test]
fn hide_attribute_and_display_none_are_dropped() {
    // source: OALD9 <span class="res-g" hide="y" psg="top_topics">
    assert_eq!(
        html_to_markdown(r##"<span hide="y">hidden</span>visible"##),
        "visible\n"
    );
    // source: OALD9's lightbox keeps a full-size <img> hidden next to the thumb
    assert_eq!(
        html_to_markdown(r##"<img style="display:none;" src="full.jpg"><img class="thumb" src="thumb.jpg">"##),
        "![thumb](thumb.jpg)\n"
    );
}

#[test]
fn comments_are_dropped() {
    // OALD9 carries 848,851 of these.
    let html = "<div>a<!-- End of DIV clear-->\n<!-- End of DIV entry--></div>";
    assert_eq!(html_to_markdown(html), "a\n");
}

/// Every piece of OALD9's web-app chrome uses `nattr="bk-not-ref-href"` plus an
/// `ohref` instead of a real `href`, so "no usable href → unwrap the text"
/// removes all of it, and the chrome classes remove the labels.
#[test]
fn oald9_chrome_is_absent() {
    // source: OALD9 "apple" top-g
    let html = r##"<div class="top-g">
 <a class="oxford3000" nattr="bk-not-ref-href" ohref="/wordlist/english/oxford3000/">&nbsp;</a>
 <div class="clear">&nbsp;</div>
 <div class="btn icon-left-colapse wl-add">
  <a id="add-to-mywordlist-link" nattr="bk-not-ref-href" ohref="/mywordlist/">Add to my wordlist</a>
 </div>
 <a class="responsive_display_inline_on_smartphone link-right" nattr="bk-not-ref-href" ohref="#relatedentries">jump to other results</a>
 <span class="def">kept</span>
</div>
<div class="pron-link">Check pronunciation: <a nattr="bk-not-ref-href" ohref="/pronunciation/english/apple">apple</a></div>"##;
    assert_eq!(html_to_markdown(html), "kept\n");
}

#[test]
fn lightbox_label_dropped_but_thumbnail_kept() {
    // source: OALD9 "apple" #ox-enlarge
    let html = r##"<div id="ox-enlarge"><a class="topic" nattr="bk-not-ref-href" ohref="/media/english/fullsize/f/fru/fruit/fruit.jpg"><img src="/media/english/fullsize/f/fru/fruit/fruit.jpg" style="display:none;"><img class="thumb" border="1" alt="" src="/media/english/thumb/f/fru/fruit/fruit.jpg"><span class="ox-enlarge-label">enlarge image</span></a></div>"##;
    assert_eq!(
        html_to_markdown(html),
        // `alt=""` falls back to the file stem rather than staying empty.
        "![fruit](/media/english/thumb/f/fru/fruit/fruit.jpg)\n"
    );
}

#[test]
fn oxench8_button_and_glyphs_add_no_content() {
    // source: OxEnCh8 "apple". The 简 button is dropped outright; the glyphs
    // become spacers, which leave nothing behind at the start of a block.
    let html = r##"<button class="simplized-btn">简</button><span class="z"><span class="symbols-coresym">★</span></span><span class="symbols-xsym">◆</span>kept"##;
    assert_eq!(html_to_markdown(html), "kept\n");
}

#[test]
fn col_in_page_toc_dropped_but_section_title_kept() {
    // source: col "run"
    let html = r##"<div id="" class="pr-container">
<p class="wordGroup"><span class="contentTitle">1.</span><a class="nav-js" href="#COLNAMING11" rel="#NAMING1">VERB USES</a></p>
</div>
<div id="COLNAMING11" class="title trans-tip"><a title="详细释义" rel="#NAMING1" class="sp do-detail">&nbsp;</a><span onclick="detail(this);">VERB USES</span></div>"##;
    assert_eq!(html_to_markdown(html), "### VERB USES\n");
}

// --- links ------------------------------------------------------------------

#[test]
fn link_targets_are_escaped() {
    // The corpus has spaces (`hand 2.jpg`), a pipe (`file:///p|/ald8/x.jpg`)
    // and apostrophes (`entry://Adam's apple`) in hrefs.
    assert_eq!(
        html_to_markdown(r##"<a href="entry://Adam's apple">x</a>"##),
        "[x](entry://Adam%27s%20apple)\n"
    );
    assert_eq!(
        html_to_markdown(r##"<a href="hand 2.jpg">y</a>"##),
        "[y](hand%202.jpg)\n"
    );
    assert_eq!(
        html_to_markdown(r##"<a href="file:///p|/ald8/x.jpg">z</a>"##),
        "[z](file:///p%7C/ald8/x.jpg)\n"
    );
    // CJK stays readable rather than being percent-encoded.
    assert_eq!(
        html_to_markdown(r##"<a href="sound://audio/苹果.mp3">a</a>"##),
        "[a](sound://audio/苹果.mp3)\n"
    );
}

/// A self-link to a fragment of the same entry. Both spellings occur: the CLI
/// path sees `entry://#HASH_word`, the server path sees `/lookup?word=#HASH_word`
/// after `rewrite_urls`.
#[test]
fn fragment_self_links_are_unwrapped() {
    assert_eq!(
        html_to_markdown(r##"<a href="entry://#49f1b0_apple">noun</a>"##),
        "noun\n"
    );
    assert_eq!(
        html_to_markdown(r##"<a href="/lookup?word=#49f1b0_apple">noun</a>"##),
        "noun\n"
    );
    // col's in-page table of contents
    assert_eq!(
        html_to_markdown(r##"<a href="#COLNAMING11">VERB USES</a>"##),
        "VERB USES\n"
    );
}

/// `/topic/...`, `/mywordlist/`, `/definition/...` are oxford.com paths that this
/// server does not route; only `/res/<dict>/...` and `/lookup...` resolve here.
#[test]
fn unroutable_root_relative_links_are_unwrapped() {
    // source: OALD9 "apple" — <a class="Ref" href="/topic/fruit/apple">
    assert_eq!(
        html_to_markdown(r##"<a class="Ref" href="/topic/fruit/apple">Fruit</a>"##),
        "Fruit\n"
    );
    assert_eq!(
        html_to_markdown(r##"<a href="/res/ox/a.mp3">play</a>"##),
        "[play](/res/ox/a.mp3)\n"
    );
    assert_eq!(
        html_to_markdown(r##"<a href="/lookup?word=crab+apple">crab apple</a>"##),
        "[crab apple](/lookup?word=crab+apple)\n"
    );
}

#[test]
fn audio_link_with_an_icon_only_label() {
    // source: CALD4 "apple" — <a href="sound://ukappen014.spx"><img src="snd_uk.png"></a>
    assert_eq!(
        html_to_markdown(r##"<a href="sound://ukappen014.spx"><img src="snd_uk.png" style="margin-bottom:-4px" border="0"></img></a>"##),
        "[audio](sound://ukappen014.spx)\n"
    );
    // source: OALD9 — a.sound is padded with &nbsp; and has a type attribute
    assert_eq!(
        html_to_markdown(r##"<a class="sound audio_play_button" type="sound" href="sound://a.mp3">&nbsp;</a>"##),
        "[audio](sound://a.mp3)\n"
    );
}

/// oxfordPrimary links the headword itself to its recording, and the whole
/// `Sound`/`Sound-en_GB` dictionaries are text links to audio. That text must
/// not be replaced by the word "audio".
#[test]
fn audio_link_with_a_text_label_keeps_the_text() {
    // source: Sound-en_GB "apple"
    assert_eq!(
        html_to_markdown(r##"<a href="sound://apple_Uk.spx">&nbsp;英式发音apple&nbsp;</a>"##),
        "[英式发音apple](sound://apple_Uk.spx)\n"
    );
}

// --- escaping ---------------------------------------------------------------

#[test]
fn a_mid_text_hyphen_is_never_escaped() {
    assert_eq!(html_to_markdown("<p>well-known</p>"), "well-known\n");
}

/// XinNiujin numbers its senses `<b>1. </b>` and COBUILD8 numbers them `1)`;
/// at the start of a line either would be read as an ordered-list marker.
#[test]
fn line_start_ordered_markers_are_escaped() {
    // source: COBUILD8 "apple" — <br>1) a rosaceous tree
    assert_eq!(
        html_to_markdown("<p>a<br>1) (intr) to move</p>"),
        "a  \n1\\) (intr) to move\n"
    );
    assert_eq!(html_to_markdown("<p>1. the round fruit</p>"), "1\\. the round fruit\n");
    // A sense number inside bold is not at line start, so it needs no escape.
    assert_eq!(
        html_to_markdown("<b>1. </b><font>the round fruit</font>"),
        "**1.** the round fruit\n"
    );
}

#[test]
fn brackets_in_pos_codes_stay_readable() {
    // source: CALD4 "apple" — [<font color="green"><b>C</b></font> or <b>U</b>]
    assert_eq!(
        html_to_markdown("[<font color=\"green\"><b>C</b></font> or <font color=\"green\"><b>U</b></font>]"),
        "[**C** or **U**]\n"
    );
}

// --- dictionary semantics ---------------------------------------------------

/// `data/OALD9.css:76` hides `.pron-gs .wrap`, so `.separator` is the visible
/// slash; line 38 hides `.bre`/`.name`, which duplicate the `.prefix` label.
/// Getting either backwards yields "BrE BrE//ˈæpl//".
#[test]
fn oald9_pronunciation_has_one_slash_and_one_label() {
    // source: OALD9 "apple"
    let html = r##"<div class="pron-gs ei-g" eid="apple__10">
 <span class="pron-g" id="apple__11" geo="br" trans="ald8">
  <span class="prefix">BrE</span>
  <span class="phon" eid="apple__12"><span class="bre">BrE</span><span class="separator">/</span><span class="wrap">/</span>ˈæpl<span class="wrap">/</span><span class="separator">/</span></span>
  <a class="sound audio_play_button pron-uk icon-audio" href="sound://media/english/uk_pron/a/app/apple/apple__gb_1.mp3" type="sound">&nbsp;</a>
 </span>
 <span class="sep">;</span>
 <span class="pron-g" id="apple__13" trans="ald8" geo="n_am">
  <span class="prefix">NAmE</span>
  <span class="phon" eid="apple__14"><span class="name">NAmE</span><span class="separator">/</span><span class="wrap">/</span>ˈæpl<span class="wrap">/</span><span class="separator">/</span></span>
  <a class="sound audio_play_button pron-us icon-audio" href="sound://media/english/us_pron/a/app/apple/apple__us_1.mp3" type="sound">&nbsp;</a>
 </span>
</div>"##;
    let md = html_to_markdown(html);
    assert_eq!(
        md,
        "BrE /ˈæpl/ [audio](sound://media/english/uk_pron/a/app/apple/apple__gb_1.mp3) ; \
         NAmE /ˈæpl/ [audio](sound://media/english/us_pron/a/app/apple/apple__us_1.mp3)\n"
    );
    // `sound://` legitimately contains "//", so check for the specific doubling.
    assert!(!md.contains("//ˈ"), "duplicated slash: {md:?}");
    assert!(!md.contains("BrE BrE"), "duplicated label: {md:?}");
}

/// `.wrap` outside `.pron-gs` supplies the real parentheses of `.label-g` and
/// must survive.
#[test]
fn oald9_label_parentheses_are_kept() {
    // source: OALD9 "apple" — the idiom's register label
    let html = r##"<span class="label-g"><span class="wrap">(</span><span class="reg" reg="saying">saying</span><span class="sep">,</span> <span class="geo" geo="n_am">especially North American English</span><span class="wrap">)</span></span> <span class="def">a child usually behaves in a similar way</span>"##;
    assert_eq!(
        html_to_markdown(html),
        "(saying, especially North American English) a child usually behaves in a similar way\n"
    );
}

/// `data/OALD9.css:138` declares `.collapse .body{display:none}` because the real
/// app reveals those panels with a JS class toggle that no stored record carries.
/// A generic CSS `display:none` rule would silently delete every "Word Origin",
/// "Extra examples" and "Collocations" body — the most valuable text there is.
#[test]
fn oald9_collapsed_panel_body_is_kept() {
    // source: OALD9 "apple"
    let html = r##"<span class="res-g"><span class="collapse" otitle="Word Origin"><span class="unbox" unbox="word_origin" id="apple__44"><span class="heading" onclick="if(this.parentNode.className=='unbox'){...}">Word Origin</span><span class="body" id="apple__45"><span class="def" id="apple__46"><span class="lang" id="apple__47">Old English</span></span> <span class="etym_i" id="apple__48">æppel</span>, of <span class="lang">Germanic</span> origin.</span></span></span></span>"##;
    let md = html_to_markdown(html);
    assert_eq!(md, "### Word Origin\n\nOld English æppel, of Germanic origin.\n");
}

/// The stylesheet hides `li.sn-g>.num` and regenerates it with a CSS counter,
/// which Markdown has no equivalent for — the stored text is the real number.
#[test]
fn oald9_sense_numbers_are_kept() {
    // source: OALD9 "run"
    let html = r##"<span class="sn-gs"><li class="sn-g"><span class="num">1 </span><span class="def">to move using your legs</span></li><li class="sn-g"><span class="num">2 </span><span class="def">to travel a particular distance</span></li></span>"##;
    assert_eq!(
        html_to_markdown(html),
        "1 to move using your legs\n\n2 to travel a particular distance\n"
    );
}

#[test]
fn oald9_examples_are_a_tight_bullet_list() {
    // source: OALD9 "apple"
    let html = r##"<span class="x-gs" id="apple__18">
 <span class="x-g" id="apple__19"><span class="rx-g"><span class="x" id="apple__21">Peel and core the apples.</span></span></span>
 <span class="x-g" id="apple__24"><span class="x" id="apple__25">an apple pie</span></span>
 <span class="x-g" id="apple__26"><span class="x" id="apple__27">apple sauce</span></span>
</span>"##;
    assert_eq!(
        html_to_markdown(html),
        "- Peel and core the apples.\n- an apple pie\n- apple sauce\n"
    );
}

#[test]
fn oald9_cross_references_keep_their_links_and_commas() {
    // source: OALD9 "apple"
    let html = r##"<span class="xr-gs"><span class="prefix">see also</span> <a class="Ref" href="entry://Adam's apple"><span class="xr-g"><span class="xh">Adam’s apple</span></span></a><span class="sep">,</span> <a class="Ref" href="entry://crab apple"><span class="xr-g"><span class="xh">crab apple</span></span></a></span>"##;
    assert_eq!(
        html_to_markdown(html),
        "see also [Adam’s apple](entry://Adam%27s%20apple), [crab apple](entry://crab%20apple)\n"
    );
}

#[test]
fn oald9_idioms_are_bold_lead_lines() {
    // source: OALD9 "apple"
    let html = r##"<span class="idm-gs"><span class="heading">Idioms</span><span class="idm-g" sk="appleofeye"><span class="idm" id="apple__73">the apple of somebody’s eye</span><span class="sn-gs"><span class="sn-g"><span class="def">a person or thing that is loved more than any other</span></span></span></span></span>"##;
    assert_eq!(
        html_to_markdown(html),
        "### Idioms\n\n**the apple of somebody’s eye**\n\na person or thing that is loved more than any other\n"
    );
}

/// OxEnCh8 renders `BrE`/`NAmE` and the audio icon with CSS margins and no
/// whitespace, so without padding this comes out as
/// `/ˈæpl[audio](…);NAmEˈæpl[audio](…)/`.
/// OALD9's Word Family box is a run of adjacent `.wfw-g` rows with no whitespace
/// between them; left inline they glue into
/// "able adjective (≠unable)ably adverbability noun".
#[test]
fn oald9_word_family_rows_are_separate_blocks() {
    // source: OALD9 "able"
    let html = r##"<span class="wfw-g" id="able_1__131"><span class="wfw">able</span> <span class="wfp" wfp="adj">adjective</span> <span class="wfo"><span class="wrap">(</span><span class="sym_first">≠</span>unable<span class="wrap">)</span></span></span><span class="wfw-g" id="able_1__134"><span class="wfw">ably</span> <span class="wfp" wfp="adv">adverb</span></span>"##;
    assert_eq!(
        html_to_markdown(html),
        "able adjective (≠unable)\n\nably adverb\n"
    );
}

#[test]
fn oxench8_pronunciation_is_padded() {
    // source: OxEnCh8 "apple"
    let html = r##"<span type="h_full_" level="3" class="ei-g"><span class="z_ei-g">/</span><span class="phon-gb">ˈæpl</span><a type="sound" class="fayin" href="sound://uk/apple__gb_1.mp3"><img src="uk_pron.png" class="fayin"/></a><span class="z">;<span class="z_phon-us">NAmE</span></span><span class="phon-us">ˈæpl</span><a type="sound" class="fayin" href="sound://us/apple__us_1.mp3"><img src="us_pron.png" class="fayin"/></a><span class="z_ei-g">/</span></span>"##;
    let md = html_to_markdown(html);
    assert!(md.contains("/ˈæpl [audio](sound://uk/apple__gb_1.mp3)"), "{md:?}");
    assert!(md.contains("; NAmE ˈæpl"), "{md:?}");
    assert!(!md.contains("NAmEˈæpl"), "unpadded: {md:?}");
}

/// `.no-dot-h` ("banana") and `.h` ("ba·nana") are the same headword with and
/// without syllable dots; only one may survive.
#[test]
fn oxench8_headword_appears_once() {
    // source: OxEnCh8 "banana"
    let html = r##"<span class="entry"><span class="no-dot-h">banana</span><span level="1" class="h-g"><span level="2" class="top-g"><span core="y" level="3" class="h">ba·nana</span></span></span></span>"##;
    assert_eq!(html_to_markdown(html), "## banana\n");
}

#[test]
fn oxench8_definition_and_translation_are_separate_blocks() {
    // source: OxEnCh8 "apple"
    let html = r##"<span level="2" class="def-g"><span status="6" level="3" class="d">a round fruit with shiny red or green skin<span localeuidoupc="201" class="chn">蘋果</span></span></span>"##;
    assert_eq!(
        html_to_markdown(html),
        "a round fruit with shiny red or green skin\n\n蘋果\n"
    );
}

/// OxEnCh8 has no whitespace between any of its inline spans — all separation is
/// CSS margins. Without padding, its grammar frames and register labels glue
/// onto their neighbours.
#[test]
fn oxench8_labels_and_frames_are_padded() {
    // source: OxEnCh8 "take" — <span class="gr">[transitive]</span><span class="cf">take sb/sth</span>
    let html = r##"<span class="gr"><span class="z_gr_br">[</span>transitive<span class="z_gr_br">]</span></span><span level="4" class="cf">take sb/sth</span>"##;
    assert_eq!(html_to_markdown(html), "[transitive] take sb/sth\n");

    // source: OxEnCh8 "way" — a register label glued to its example
    let html2 = r##"<span class="x-g"><span class="symbols-xsym">◆</span><span level="5" class="label-g">(<span level="6" r="fig" class="r">figurative</span>)</span><span level="5" class="x">Her life had always run smoothly.</span></span>"##;
    assert_eq!(
        html_to_markdown(html2),
        "- (figurative) Her life had always run smoothly.\n"
    );

    // source: OxEnCh8 "go" — an inline gloss inside an example sentence
    let html3 = r##"<span class="x">She decided not to go through with<span level="6" class="gl">(= not to have)</span>the operation.</span>"##;
    assert_eq!(
        html_to_markdown(html3),
        "She decided not to go through with (= not to have) the operation.\n"
    );
}

/// A decoration glyph is dropped, but the CSS margin around it was doing real
/// work, so it leaves a space rather than gluing its neighbours together.
#[test]
fn oxench8_glyph_spans_leave_a_space() {
    // source: OxEnCh8 "go" — ★ marks an Oxford-3000 core word
    let html = r##"<span class="z_n">1.</span><span class="symbols-small_coresym">★</span><span class="gr">[intransitive]</span>"##;
    assert_eq!(html_to_markdown(html), "1\\. [intransitive]\n");
    // A spacer at the start of a block adds no leading space.
    assert_eq!(
        html_to_markdown(r##"<span class="symbols-xsym">◆</span>kept"##),
        "kept\n"
    );
}

/// The `symbols-*` spans that hold a word rather than a glyph are content.
#[test]
fn oxench8_word_labels_are_kept() {
    // source: OxEnCh8 "go" — <span class="symbols-helpsym">HELP</span>
    let html = r##"<span class="help"><span class="z"><span class="symbols-helpsym">HELP</span></span><span class="d">Been is used as the past participle.</span></span>"##;
    assert_eq!(
        html_to_markdown(html),
        "HELP Been is used as the past participle.\n"
    );
}

#[test]
fn oxench8_span_table_cells_are_joined_not_piped() {
    // source: OxEnCh8 "big" — a span-based layout table that nests arbitrarily
    // deep, which a GFM pipe table cannot represent.
    let html = r##"<span class="table"><span class="tr"><span class="th">big ~</span><span class="th">large ~</span></span><span class="tr"><span class="td">man</span><span class="td">numbers</span></span></span>"##;
    let md = html_to_markdown(html);
    assert_eq!(md, "big ~ | large ~\n\nman | numbers\n");
    assert!(!md.contains("|-"), "GFM separator row: {md:?}");
}

#[test]
fn col_sense_number_joins_the_definition_line() {
    // source: col "run"
    let html = r##"<ul class="ol"><li><div class="collinsMajorTrans"><span class="collinsOrder">1</span><p><span class="additional" title="及物动词">V-T</span> When you <b>run</b>, you move more quickly.</p></div></li></ul>"##;
    assert_eq!(
        html_to_markdown(html),
        "- **1** V-T When you **run**, you move more quickly.\n"
    );
}

#[test]
fn col_examples_are_indented_under_their_sense() {
    // source: col "apple"
    let html = r##"<div class="exampleLists"><div class="examples"><p> I want an apple. </p><p>我要一个苹果。</p></div></div>"##;
    assert_eq!(html_to_markdown(html), "I want an apple.\n\n我要一个苹果。\n");
}

/// COBUILD8's markup is deliberately misnested; the text must still survive.
#[test]
fn cobuild_misnested_tags() {
    // source: COBUILD8, the banner every record opens with
    let html = "<br><font color=green size=5><b>《柯林斯英英词典第8版》</font></b>";
    assert_eq!(html_to_markdown(html), "**《柯林斯英英词典第8版》**\n");
    // source: COBUILD8 "abbreviation"
    // html5ever repairs the misnesting into two nested <i>, hence `**`.
    let html2 = "<font color=green><I><font color=green><I>abbreviation for </font></I> </font></I>";
    assert_eq!(html_to_markdown(html2), "**abbreviation for**\n");
}

/// XinNiujin's `<frac>` has no whitespace between numerator and denominator, so
/// transparent rendering would produce the misleading `1299 792 458`.
#[test]
fn xinniujin_fraction() {
    assert_eq!(
        html_to_markdown("<frac><nu>1</nu><dn>299 792 458</dn></frac>"),
        "1/299 792 458\n"
    );
}

#[test]
fn xinniujin_custom_tags_are_transparent() {
    // <ll>, <xr>, <xrg> and <tx> are not HTML; their text is real content.
    assert_eq!(
        html_to_markdown(r##"<ll><xr>rise</xr> text</ll>"##),
        "rise text\n"
    );
}

/// Uppercase tag spellings appear throughout COBUILD8 and XinNiujin. html5ever
/// lowercases HTML tag names, so these must behave exactly like their lowercase
/// forms.
#[test]
fn uppercase_tags_are_handled() {
    assert_eq!(
        html_to_markdown("<FONT color=red>a</FONT><BR><I>b</I><DIV>c</DIV>"),
        "a  \n*b*\n\nc\n"
    );
}

// --- redirect stubs ---------------------------------------------------------

/// 24,298 OALD9 records (29% of the dictionary) are bare `@@@LINK=` stubs.
/// `lookup::Dict::lookup_word` resolves them; this covers one that survives.
#[test]
fn at_link_stub_renders_as_a_redirect() {
    assert_eq!(html_to_markdown("@@@LINK=will"), "→ will\n");
    assert_eq!(html_to_markdown("@@@LINK=will\r\n"), "→ will\n");
}

#[test]
fn at_link_stub_mixed_with_markup_is_left_to_the_parser() {
    // The caller joins multi-slot records with `<hr/>`, so a stub is only a stub
    // when the control string is the whole record.
    let md = html_to_markdown("@@@LINK=abase<hr/>@@@LINK=abase");
    assert!(md.contains("@@@LINK=abase"), "{md:?}");
}

#[test]
fn decode_record_strips_the_trailing_nul() {
    // Every one of the 762,147 records in the corpus ends "\r\n\x00"; left in
    // place the parser turns it into U+FFFD at the end of every entry.
    assert_eq!(decode_record(b"<p>apple</p>\r\n\x00"), "<p>apple</p>\r\n");
    assert_eq!(html_to_markdown("<p>apple</p>\r\n\x00"), "apple\n");
    assert!(!html_to_markdown("<p>apple</p>\x00").contains('\u{fffd}'));
}

// --- robustness -------------------------------------------------------------

/// `server.rs` converts inside `spawn_blocking` and falls back to raw HTML if the
/// task panics. This keeps that fallback unreachable.
#[test]
fn never_panics_on_truncated_input() {
    let html = REAL_RECORD;
    for i in 0..html.len() {
        if i % 97 != 0 {
            continue;
        }
        let md = html_to_markdown(&html[..i]);
        assert!(!md.contains('\u{fffd}'), "replacement char at {i}: {md:?}");
    }
    // Truncating mid-character must not panic either.
    for i in 0..html.len() {
        if html.is_char_boundary(i) {
            html_to_markdown(&html[..i]);
        }
    }
}

#[test]
fn pathological_nesting_hits_the_depth_cap_without_panicking() {
    // Past the cap content is dropped rather than risking a stack overflow. The
    // deepest real record measures 15 levels (OALD9 "hand"), so this only fires
    // on a pathological dictionary; the contract here is "never panic".
    for depth in [1, 16, 127, 128, 129, 1000, 5000] {
        let html = format!("{}x{}", "<div>".repeat(depth), "</div>".repeat(depth));
        html_to_markdown(&html);
    }
    assert_eq!(html_to_markdown("<div><div>x</div></div>"), "x\n");
}

#[test]
fn empty_and_blank_inputs() {
    assert_eq!(html_to_markdown(""), "");
    assert_eq!(html_to_markdown("\u{0}"), "");
    assert_eq!(html_to_markdown("<div></div><span>&nbsp;</span>"), "");
    assert_eq!(html_to_markdown("<!-- nothing -->"), "");
}

/// A real OALD9 record, trimmed, used by the truncation test when the fixture
/// file is absent.
const REAL_RECORD: &str = r##"<a name="49f1b05b3013431b8dbb9ff989c2755a_apple" nattr="head-jump-entry-flag"></a>
<div xmlns="http://www.w3.org/1999/xhtml" class="entry" id="apple">
 <div class="h-g" id="apple__1">
  <div class="top-container">
   <div class="top-g" id="apple__2">
    <div class="webtop-g">
     <a class="oxford3000" nattr="bk-not-ref-href" ohref="/wordlist/english/oxford3000/">&nbsp;</a>
     <span class="z">&nbsp;</span>
     <h2 class="h">apple</h2>
     <a class="pos" href="entry://#49f1b05b3013431b8dbb9ff989c2755a_apple" nattr="head-jump-entry-content">noun</a>
    </div>
    <span class="pos-g" id="apple__8"><span class="pos" id="apple__9">noun</span></span>
    <div class="pron-gs ei-g">
     <span class="pron-g" geo="br">
      <span class="prefix">BrE</span>
      <span class="phon"><span class="bre">BrE</span><span class="separator">/</span><span class="wrap">/</span>ˈæpl<span class="wrap">/</span><span class="separator">/</span></span>
      <a class="sound audio_play_button pron-uk icon-audio" href="sound://media/english/uk_pron/a/app/apple/apple__gb_1.mp3" type="sound">&nbsp;</a>
     </span>
     <span class="sep">;</span>
    </div>
    <div class="btn icon-left-colapse wl-add"><a id="add-to-mywordlist-link" nattr="bk-not-ref-href" ohref="/mywordlist/">Add to my wordlist</a></div>
    <a class="responsive_display_inline_on_smartphone link-right" nattr="bk-not-ref-href" ohref="#relatedentries">jump to other results</a>
    <br>
   </div>
  </div>
  <span class="sn-gs" id="apple__15">
   <li class="sn-g" id="apple__16">
    <div id="ox-enlarge">
     <a class="topic" nattr="bk-not-ref-href" ohref="/media/english/fullsize/f/fru/fruit/fruit.jpg">
      <img src="/media/english/fullsize/f/fru/fruit/fruit.jpg" style="display:none;">
      <img class="thumb" border="1" alt="" src="/media/english/thumb/f/fru/fruit/fruit.jpg">
      <span class="ox-enlarge-label">enlarge image</span>
     </a>
    </div>
    <span class="def" id="apple__17">a round fruit with shiny red or green skin and firm white flesh</span>
    <span class="x-gs" id="apple__18">
     <span class="x-g" id="apple__19"><span class="x" id="apple__21">Peel and core the apples.</span></span>
     <span class="x-g" id="apple__24"><span class="x" id="apple__25">an apple pie</span></span>
    </span>
    <span class="xr-gs" id="apple__30">
     <span class="prefix">see also</span> <a class="Ref" href="entry://Adam's apple"><span class="xr-g"><span class="xh">Adam’s apple</span></span></a><span class="sep">,</span> <a class="Ref" href="entry://crab apple"><span class="xr-g"><span class="xh">crab apple</span></span></a>
    </span>
   </li>
  </span>
  <span class="res-g">
   <span class="collapse" otitle="Word Origin">
    <span class="unbox" unbox="word_origin">
     <span class="heading">Word Origin</span>
     <span class="body"><span class="def"><span class="lang">Old English</span></span> <span class="etym_i">æppel</span>, of <span class="lang">Germanic</span> origin.</span>
    </span>
   </span>
  </span>
 </div>
</div>
<link rel="stylesheet" type="text/css" href="OALD9.css">"##;

/// The whole trimmed record at once: the interactions between the headword, the
/// part of speech, both pronunciations, the sense, its examples, the cross
/// references and a collapsed panel.
#[test]
fn full_oald9_entry() {
    assert_eq!(
        html_to_markdown(REAL_RECORD),
        concat!(
            "## apple\n\n",
            "noun\n\n",
            "BrE /ˈæpl/ [audio](sound://media/english/uk_pron/a/app/apple/apple__gb_1.mp3) ;\n\n",
            "![fruit](/media/english/thumb/f/fru/fruit/fruit.jpg)\n\n",
            "a round fruit with shiny red or green skin and firm white flesh\n\n",
            "- Peel and core the apples.\n",
            "- an apple pie\n\n",
            "see also [Adam’s apple](entry://Adam%27s%20apple), [crab apple](entry://crab%20apple)\n\n",
            "### Word Origin\n\n",
            "Old English æppel, of Germanic origin.\n",
        )
    );
}
