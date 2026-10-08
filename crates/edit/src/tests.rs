use std::sync::Arc;

use printcraft_cos::{Document, SaveOptions, write_incremental};

use super::*;

/// Three pages: an upright page with content that leaves the graphics state changed (an
/// unbalanced `cm`), a page rotated 90° and one with inherited, shared resources.
fn fixture() -> Document {
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 600 800] /Resources 6 0 R >>".into(),
        "<< /Type /Page /Parent 2 0 R /Contents 7 0 R >>".into(),
        "<< /Type /Page /Parent 2 0 R /Rotate 90 /Contents [7 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R >>".into(),
        "<< /Font << /F1 8 0 R >> >>".into(),
        "<< /Length 37 >>\nstream\n2 0 0 2 0 0 cm BT /F1 9 Tf (Hi) Tj ET\nendstream".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    Document::open(Arc::new(out)).unwrap()
}

#[test]
fn spreadsheet_image_coordinates_follow_crop_and_covered_pixels_are_not_exported() {
    let mut d = fixture();
    let p = printcraft_model::pages(&d)[0].obj;
    let mut dict = Dict::new();
    dict.set(b"Type".to_vec(), Object::name("XObject"));
    dict.set(b"Subtype".to_vec(), Object::name("Image"));
    dict.set(b"Width".to_vec(), 1i64);
    dict.set(b"Height".to_vec(), 1i64);
    dict.set(b"BitsPerComponent".to_vec(), 8i64);
    dict.set(b"ColorSpace".to_vec(), Object::name("DeviceRGB"));
    let image = d.add(Object::Stream(Stream::from_raw(dict, vec![12, 32, 55])));
    let mut objects = Dict::new();
    objects.set(b"Im".to_vec(), Object::Ref(image));
    let mut resources = Dict::new();
    resources.set(b"XObject".to_vec(), Object::Dict(objects));
    let source = b"q 100 0 0 20 30 710 cm /Im Do Q";
    let content = d.add(Object::Stream(Stream::flate(Dict::new(), source)));
    d.update_dict(p, |p| {
        p.set(b"Resources".to_vec(), Object::Dict(resources));
        p.set(b"CropBox".to_vec(), Object::Array([20, 50, 520, 750].into_iter().map(Object::Int).collect()));
        p.set(b"Contents".to_vec(), Object::Ref(content));
    })
    .unwrap();
    let mut copy = d.clone();
    let layout = sheet_export::layout(&mut copy, 0).unwrap();
    assert!(!layout.complex);
    assert_eq!(layout.images, [[10.0, 20.0, 110.0, 40.0]]);
    let covered = d.add(Object::Stream(Stream::flate(Dict::new(), b"q 100 0 0 20 30 710 cm /Im Do Q 1 g 30 710 100 20 re f")));
    d.update_dict(p, |p| {
        p.set(b"Contents".to_vec(), Object::Ref(covered));
    })
    .unwrap();
    assert!(sheet_export::layout(&mut d, 0).unwrap().complex);
}

#[test]
fn spreadsheet_reads_cross_stream_styles_lines_fills_and_ignores_invisible_text() {
    let mut original = fixture();
    let p = printcraft_model::pages(&original)[0].obj;
    let a = original.add(Object::Stream(Stream::flate(
        Dict::new(),
        b"q 0.9 0.95 1 rg 20 600 100 50 re f 0.2 0.4 0.6 RG 1.5 w 20 600 m 120 600 l S BT /F1 12 Tf",
    )));
    let b = original.add(Object::Stream(Stream::flate(
        Dict::new(),
        b"0.2 0.3 0.4 rg 25 620 Td (Visible) Tj ET Q BT /F1 12 Tf 3 Tr 20 400 Td (Hidden OCR) Tj ET",
    )));
    original
        .update_dict(p, |d| {
            d.set(b"Contents".to_vec(), Object::Array(vec![Object::Ref(a), Object::Ref(b)]));
        })
        .unwrap();
    let before = streams(&original, 0);
    let mut copy = original.clone();
    let layout = sheet_export::layout(&mut copy, 0).unwrap();
    assert!(!layout.complex);
    assert_eq!(layout.text.len(), 1);
    assert_eq!(layout.text[0].line.text, "Visible");
    assert_eq!(layout.rules[0].color, [0.2, 0.4, 0.6]);
    assert_eq!(layout.rules[0].width, 1.5);
    assert_eq!(layout.fills.len(), 1);
    assert!((layout.text[0].rect[0] - 25.0).abs() < 0.01);
    assert_eq!(streams(&original, 0), before);
}

#[test]
fn spreadsheet_falls_back_for_occluded_clipped_rotated_and_transparent_content() {
    for content in [
        "BT /F1 12 Tf 20 680 Td (Secret) Tj ET 1 g 15 670 100 30 re f",
        "q 0 0 10 10 re W n BT /F1 12 Tf 20 680 Td (Clipped) Tj ET Q",
        "BT /F1 12 Tf 0.866 0.5 -0.5 0.866 100 150 Tm (DRAFT) Tj ET",
        "q /Unknown gs BT /F1 12 Tf 20 680 Td (Transparent) Tj ET Q",
        "0 0 m 10 10 20 20 30 40 c S",
    ] {
        let mut d = fixture();
        let p = printcraft_model::pages(&d)[0].obj;
        let r = d.add(Object::Stream(Stream::flate(Dict::new(), content.as_bytes())));
        d.update_dict(p, |page| {
            page.set(b"Contents".to_vec(), Object::Ref(r));
        })
        .unwrap();
        let layout = sheet_export::layout(&mut d, 0).unwrap();
        assert!(layout.complex || layout.text.is_empty(), "{content}");
    }
}

#[test]
fn word_background_preserves_relative_advances_and_keeps_slanted_watermarks() {
    let mut doc = fixture();
    let page = printcraft_model::pages(&doc)[0].obj;
    let content = b"q BT /F1 12 Tf 1 0 0 1 20 680 Tm (First) Tj (Second) Tj ET Q\nq BT /F1 30 Tf 0.866 0.5 -0.5 0.866 100 150 Tm (DRAFT) Tj ET Q\nBT /F1 12 Tf 3 Tr 20 600 Td (invisible OCR) Tj ET\nBT /F1 12 Tf 7 Tr 20 570 Td (clip text) Tj ET";
    let r = doc.add(Object::Stream(Stream::flate(Dict::new(), content)));
    doc.update_dict(page, |d| {
        d.set(b"Contents".to_vec(), Object::Ref(r));
    })
    .unwrap();
    let before = text_lines(&doc, 0).unwrap();
    let editable = word_export::hide_editable_text(&mut doc, 0).unwrap();
    assert_eq!(editable.len(), 1);
    assert_eq!(editable[0].line.text, "FirstSecond");
    assert_eq!(editable[0].line.base_font, "Courier");
    assert_eq!(editable[0].size, 12.0);
    assert!((editable[0].rect[0] - 20.0).abs() < 0.01);
    let after = text_lines(&doc, 0).unwrap();
    assert_eq!(
        before.iter().map(|l| (&l.text, l.rect)).collect::<Vec<_>>(),
        after.iter().map(|l| (&l.text, l.rect)).collect::<Vec<_>>(),
        "invisible text still advances; no implicit positions are lost"
    );
    let background = streams(&doc, 0).join("\n");
    assert!(background.contains("(DRAFT) Tj"));
    assert!(background.contains("3 Tr\n(First) Tj\n0 Tr\n3 Tr\n(Second) Tj\n0 Tr"), "{background}");
}

#[test]
fn word_export_graphics_state_spans_streams_and_shared_originals_are_not_changed() {
    let mut original = fixture();
    let page = printcraft_model::pages(&original)[0].obj;
    let a = original.add(Object::Stream(Stream::flate(Dict::new(), b"q 3 Tr")));
    let b =
        original.add(Object::Stream(Stream::flate(Dict::new(), b"BT /F1 12 Tf 20 700 Td (Hidden) Tj ET Q BT /F1 12 Tf 20 600 Td (Visible) Tj ET")));
    original
        .update_dict(page, |d| {
            d.set(b"Contents".to_vec(), Object::Array(vec![Object::Ref(a), Object::Ref(b)]));
        })
        .unwrap();
    let before = streams(&original, 0);
    let mut copy = original.clone();
    let editable = word_export::hide_editable_text(&mut copy, 0).unwrap();
    assert_eq!(editable.iter().map(|t| t.line.text.as_str()).collect::<Vec<_>>(), ["Visible"]);
    assert_eq!(streams(&original, 0), before);
}

#[test]
fn word_export_keeps_clipped_symbols_and_uses_cross_stream_transforms() {
    let mut doc = fixture();
    let page = printcraft_model::pages(&doc)[0].obj;
    let a = doc.add(Object::Stream(Stream::flate(Dict::new(), b"q 0 0 600 800 re W n 2 0 0 2 0 0 cm BT /F1 12 Tf")));
    let b = doc.add(Object::Stream(Stream::flate(Dict::new(), b"20 300 Td (Scaled) Tj ET Q q 0 0 10 10 re W n BT /F1 12 Tf 20 200 Td (Clipped) Tj ET Q BT /F2 12 Tf 20 180 Td (Symbol) Tj ET BT /F1 12 Tf -10 150 Td (Outside) Tj ET")));
    let mut symbol = Dict::new();
    symbol.set(b"Type".to_vec(), Object::name("Font"));
    symbol.set(b"Subtype".to_vec(), Object::name("Type1"));
    symbol.set(b"BaseFont".to_vec(), Object::name("Wingdings-Regular"));
    let mut fonts = Dict::new();
    fonts.set(b"F1".to_vec(), Object::Ref(printcraft_cos::ObjRef::new(8, 0)));
    fonts.set(b"F2".to_vec(), Object::Dict(symbol));
    let mut res = Dict::new();
    res.set(b"Font".to_vec(), Object::Dict(fonts));
    doc.update_dict(page, |d| {
        d.set(b"Contents".to_vec(), Object::Array(vec![Object::Ref(a), Object::Ref(b)]));
        d.set(b"Resources".to_vec(), Object::Dict(res));
    })
    .unwrap();
    let editable = word_export::hide_editable_text(&mut doc, 0).unwrap();
    assert_eq!(editable.iter().map(|t| t.line.text.as_str()).collect::<Vec<_>>(), ["Scaled"]);
    assert_eq!(editable[0].size, 24.0);
    assert!((editable[0].rect[0] - 40.0).abs() < 0.01);
    let background = streams(&doc, 0).join("\n");
    assert!(background.contains("(Clipped) Tj") && background.contains("(Symbol) Tj") && background.contains("(Outside) Tj"));
    assert!(!background.contains("3 Tr\n(Clipped)") && !background.contains("3 Tr\n(Symbol)") && !background.contains("3 Tr\n(Outside)"));
}

#[test]
fn watermark_analysis_removes_only_selected_content_and_rejects_stale_ids() {
    let mut doc = fixture();
    let page = printcraft_model::pages(&doc)[0].obj;
    let bytes = b"BT /F1 12 Tf 20 680 Td (Keep this body) Tj ET\nq /Fade gs BT /F1 40 Tf 0.707 0.707 -0.707 0.707 100 150 Tm (DRAFT) Tj ET Q\nBT /F1 12 Tf 20 640 Td (Keep this too) Tj ET";
    let stream = doc.add(Object::Stream(Stream::flate(Dict::new(), bytes)));
    let mut fade = Dict::new();
    fade.set(b"ca".to_vec(), Object::Real(0.3));
    let mut ext = Dict::new();
    ext.set(b"Fade".to_vec(), Object::Dict(fade));
    doc.update_dict(page, |d| {
        d.set(b"Contents".to_vec(), Object::Ref(stream));
        let mut res = Dict::new();
        let mut fonts = Dict::new();
        fonts.set(b"F1".to_vec(), Object::Ref(printcraft_cos::ObjRef::new(8, 0)));
        res.set(b"Font".to_vec(), Object::Dict(fonts));
        res.set(b"ExtGState".to_vec(), Object::Dict(ext));
        d.set(b"Resources".to_vec(), Object::Dict(res));
    })
    .unwrap();
    let found = watermarks::analyze(&doc, &[0], false).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label, "DRAFT");
    let selected = vec![found[0].id.clone()];
    assert_eq!(watermarks::remove(&mut doc, &[0], &selected).unwrap(), 1);
    let text: Vec<_> = text_lines(&reopen(&doc), 0).unwrap().into_iter().map(|l| l.text).collect();
    assert_eq!(text, ["Keep this body", "Keep this too"]);
    assert!(watermarks::remove(&mut doc, &[0], &selected).is_err());
}

#[test]
fn watermark_repeated_absolute_positions_in_one_text_object_are_separable() {
    let mut doc = fixture();
    let mut bytes =
        b"BT /F1 319 Tf 0.05 0 0 0.05 20 700 Tm (Keep body) Tj ET\nBT /F1 319 Tf 0.05 0 0 0.05 20 650 Tm (\\050) Tj ET\nBT /F1 20 Tf 0.5 g\n"
            .to_vec();
    for x in [0, 198, 396] {
        for y in [0, 210, 420, 630] {
            bytes.extend_from_slice(format!("0.86603 0.5 -0.5 0.86603 {x} {y} Tm (DRAFT 2026-10-08) Tj\n1 0 0 1 0 0 Tm\n").as_bytes());
        }
    }
    bytes.extend_from_slice(b"ET\nBT /F1 12 Tf 20 610 Td (Keep footer) Tj ET");
    for page in printcraft_model::pages(&doc).iter().take(2) {
        let stream = doc.add(Object::Stream(Stream::flate(Dict::new(), &bytes)));
        doc.update_dict(page.obj, |d| d.set(b"Contents".to_vec(), Object::Ref(stream))).unwrap();
    }
    let before: Vec<_> = text_lines(&doc, 0).unwrap().into_iter().filter(|l| !l.text.contains("DRAFT")).map(|l| (l.text, l.rect)).collect();
    let found = watermarks::analyze(&doc, &[0, 1], false).unwrap();
    assert_eq!(found.len(), 1, "scaled small body text must not become a large-text candidate: {found:?}");
    assert_eq!(found[0].label, "DRAFT 2026-10-08");
    assert_eq!(found[0].occurrences.len(), 24);
    assert_eq!(watermarks::remove(&mut doc, &[0, 1], &[found[0].id.clone()]).unwrap(), 24);
    let reopened = reopen(&doc);
    for page in [0, 1] {
        let after: Vec<_> = text_lines(&reopened, page).unwrap().into_iter().map(|l| (l.text, l.rect)).collect();
        assert_eq!(after, before, "unselected text and positions must survive save/reopen");
    }
}

#[test]
fn watermark_partial_text_requires_absolute_position_on_both_sides() {
    for body in
        ["BT /F1 12 Tf 1 0 0 1 20 600 Tm (Keep) Tj /F1 30 Tf (DRAFT) Tj ET", "BT /F1 30 Tf 1 0 0 1 20 600 Tm (DRAFT) Tj /F1 12 Tf (Keep) Tj ET"]
    {
        let mut doc = fixture();
        let page = printcraft_model::pages(&doc)[0].obj;
        let stream = doc.add(Object::Stream(Stream::flate(Dict::new(), body.as_bytes())));
        doc.update_dict(page, |d| d.set(b"Contents".to_vec(), Object::Ref(stream))).unwrap();
        assert!(!watermarks::analyze(&doc, &[0], true).unwrap().iter().any(|c| c.label == "DRAFT"));
    }
}

#[test]
fn watermark_artifacts_and_image_candidates_exclude_full_page_scans_and_shared_text() {
    let mut doc = fixture();
    let page = printcraft_model::pages(&doc)[0].obj;
    let mut im = Dict::new();
    im.set(b"Subtype".to_vec(), Object::name("Image"));
    im.set(b"Width".to_vec(), Object::Int(1));
    im.set(b"Height".to_vec(), Object::Int(1));
    im.set(b"ColorSpace".to_vec(), Object::name("DeviceGray"));
    im.set(b"BitsPerComponent".to_vec(), Object::Int(8));
    let img = doc.add(Object::Stream(Stream::from_raw(im, vec![128])));
    let mut xo = Dict::new();
    xo.set(b"I".to_vec(), Object::Ref(img));
    let bytes = b"q 600 0 0 800 0 0 cm /I Do Q\nq 40 0 0 40 120 80 cm /I Do Q\nBT /F1 12 Tf 20 600 Td (Body) Tj ( DRAFT) Tj ET\n/Artifact << /Subtype /Watermark >> BDC q BT /F1 28 Tf 30 300 Td (SAMPLE) Tj ET Q EMC";
    let content = doc.add(Object::Stream(Stream::flate(Dict::new(), bytes)));
    doc.update_dict(page, |d| {
        d.set(b"Contents".to_vec(), Object::Ref(content));
        let mut res = Dict::new();
        let mut fonts = Dict::new();
        fonts.set(b"F1".to_vec(), Object::Ref(printcraft_cos::ObjRef::new(8, 0)));
        res.set(b"Font".to_vec(), Object::Dict(fonts));
        res.set(b"XObject".to_vec(), Object::Dict(xo));
        d.set(b"Resources".to_vec(), Object::Dict(res));
    })
    .unwrap();
    let found = watermarks::analyze(&doc, &[0], true).unwrap();
    assert_eq!(found.iter().filter(|c| c.kind == "image").count(), 1, "never offer the full-page scan");
    let artifact = found.iter().find(|c| c.kind == "artifact").unwrap();
    watermarks::remove(&mut doc, &[0], std::slice::from_ref(&artifact.id)).unwrap();
    assert!(text_lines(&reopen(&doc), 0).unwrap().iter().any(|l| l.text == "Body DRAFT"));
    assert!(!text_lines(&reopen(&doc), 0).unwrap().iter().any(|l| l.text == "SAMPLE"));
    assert_eq!(page_images(&doc, 0).unwrap().len(), 2);
}

fn reopen(doc: &Document) -> Document {
    let bytes = write_incremental(doc, &SaveOptions::default()).unwrap();
    hayro_syntax::Pdf::new(bytes.clone()).expect("parses");
    Document::open(Arc::new(bytes)).unwrap()
}

#[test]
fn production_edits_copy_shared_forms_and_preserve_other_pages() {
    use production::Settings;
    let mut doc = fixture();
    let mut form = Dict::new();
    form.set(b"Subtype".to_vec(), Object::name("Form"));
    form.set(b"BBox".to_vec(), Object::Array([0, 0, 100, 100].into_iter().map(Object::Int).collect()));
    let original = b"1 0 0 rg 0 w 10 10 m 90 90 l S";
    let form_ref = doc.add(Object::Stream(Stream::flate(form, original)));
    let content = doc.add(Object::Stream(Stream::flate(Dict::new(), b"q /Fm Do Q BT /F1 12 Tf 20 150 Td (KEEP) Tj ET")));
    let mut xo = Dict::new();
    xo.set(b"Fm".to_vec(), Object::Ref(form_ref));
    doc.update_dict(printcraft_cos::ObjRef::new(6, 0), |d| d.set(b"XObject".to_vec(), Object::Dict(xo))).unwrap();
    let pages = printcraft_model::pages(&doc);
    for p in &pages[..2] {
        doc.update_dict(p.obj, |d| d.set(b"Contents".to_vec(), Object::Ref(content))).unwrap();
    }
    production::apply(&mut doc, &[0], &Settings::VectorGray).unwrap();
    production::apply(&mut doc, &[0], &Settings::Hairlines { minimum: 0.5 }).unwrap();
    let doc = reopen(&doc);
    let forms: Vec<_> = printcraft_model::pages(&doc)[..2]
        .iter()
        .map(|p| {
            let res = doc.resolve(p.dict.get(b"Resources").unwrap());
            let xo = doc.resolve(res.as_dict().unwrap().get(b"XObject").unwrap());
            stream_bytes(&doc, xo.as_dict().unwrap().get(b"Fm").unwrap()).unwrap()
        })
        .collect();
    let text = String::from_utf8(forms[0].clone()).unwrap();
    assert!(text.contains("0.2126 g") && text.contains("0.5 w"), "{text}");
    assert_eq!(forms[1], original);
    assert_eq!(stream_bytes(&doc, &Object::Ref(form_ref)).unwrap(), original, "shared source untouched");
    assert_eq!(text_lines(&doc, 0).unwrap()[0].text, "KEEP");
}

#[test]
fn production_transitions_and_printer_marks_survive_save() {
    use production::Settings;
    let mut doc = fixture();
    production::apply(&mut doc, &[0], &Settings::Transitions { style: "Fade".into(), seconds: 1.5 }).unwrap();
    production::apply(&mut doc, &[0], &Settings::PrinterMarks { margin: 36.0 }).unwrap();
    let mut doc = reopen(&doc);
    let pages = printcraft_model::pages(&doc);
    assert_eq!(pages[0].crop(&doc), [-36.0, -36.0, 636.0, 836.0]);
    let transition = doc.resolve(pages[0].dict.get(b"Trans").unwrap());
    assert_eq!(transition.as_dict().unwrap().name(b"S"), Some(b"Fade".as_slice()));
    assert_eq!(pages[1].crop(&doc), [0.0, 0.0, 600.0, 800.0]);
    assert!(streams(&doc, 0).last().unwrap().contains("0.5 w"));
    let contents = doc.resolve(pages[0].dict.get(b"Contents").unwrap());
    let mark = doc.resolve(contents.as_array().unwrap().last().unwrap());
    assert!(matches!(&*mark,Object::Stream(s) if s.dict.name(b"PCMark")==Some(b"PrinterMarks")));
    assert_eq!(text_lines(&doc, 0).unwrap()[0].text, "Hi");
    production::apply(&mut doc, &[0], &Settings::Transitions { style: "none".into(), seconds: 1.0 }).unwrap();
    assert!(!printcraft_model::pages(&reopen(&doc))[0].dict.contains(b"Trans"));
    assert!(production::apply(&mut doc, &[0], &Settings::Hairlines { minimum: f64::NAN }).is_err());
    assert!(production::apply(&mut doc, &[0], &Settings::PrinterMarks { margin: 0.0 }).is_err());
}

/// The decoded content streams of a page, in order.
fn streams(doc: &Document, page: usize) -> Vec<String> {
    let p = &printcraft_model::pages(doc)[page];
    let c = p.dict.get(b"Contents").cloned();
    let list = match c.map(|c| doc.resolve(&c)).as_deref() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => vec![p.dict.get(b"Contents").cloned().unwrap()],
        None => vec![],
    };
    list.iter().map(|o| String::from_utf8_lossy(&stream_bytes(doc, o).unwrap()).into_owned()).collect()
}

fn cx() -> Context {
    Context { date: (2026, 10, 1) }
}

#[test]
fn tokens_expand() {
    let c = cx();
    assert_eq!(expand("Page <<1>> of <<n>>", 3, 10, 0, &c), "Page 3 of 10");
    assert_eq!(expand("<<1 of n>> · <<Page 1>> · <<1/n>>", 2, 5, 0, &c), "2 of 5 · Page 2 · 2/5");
    assert_eq!(expand("<<m/d/yyyy>> <<yyyy-mm-dd>> <<mmmm d, yyyy>>", 1, 1, 0, &c), "10/1/2026 2026-10-01 October 1, 2026");
    assert_eq!(expand("<<Bates Number#6#100#ABC#-X>>", 1, 1, 104, &c), "ABC000104-X");
    assert_eq!(expand("keep <<unknown>> and <<unclosed", 1, 1, 0, &c), "keep <<unknown>> and <<unclosed");
    assert_eq!(bates_start("x <<Bates Number#6#100#A#B>>"), Some(100));
}

#[test]
fn header_and_footer_are_drawn_in_display_space_and_wrap_the_original_content() {
    let mut doc = fixture();
    let hf = HeaderFooter {
        text: ["Left".into(), String::new(), "<<Page 1 of n>>".into(), String::new(), "Confidential (draft)".into(), String::new()],
        ..HeaderFooter::default()
    };
    add_header_footer(&mut doc, &[0, 1, 2], &hf, false, &cx()).unwrap();
    let doc = reopen(&doc);
    let s0 = streams(&doc, 0);
    // q-wrapper, original, Q-wrapper, header/footer.
    assert_eq!(s0.len(), 4, "{s0:?}");
    assert_eq!((s0[0].as_str(), s0[2].as_str()), ("q %PrintCraft\n", "Q %PrintCraft\n"));
    let mark = &s0[3];
    assert!(mark.contains("/PCMark /HeaderFooter") && mark.contains("(Page 1 of 3) Tj") && mark.contains("(Confidential \\(draft\\)) Tj"), "{mark}");
    assert!(mark.contains("1 0 0 1 0 0 cm"), "upright page: identity");
    // The rotated page maps display space onto its rotated crop box.
    let s1 = streams(&doc, 1);
    assert!(s1.last().unwrap().contains("0 1 -1 0 600 0 cm") && s1.last().unwrap().contains("(Page 2 of 3)"), "{s1:?}");
    // A page without content gets just the mark; its inherited resources are copied, not changed.
    assert_eq!(streams(&doc, 2).len(), 1);
    let p2 = &printcraft_model::pages(&doc)[2];
    let res = doc.resolve(p2.dict.get(b"Resources").unwrap());
    let fonts = doc.resolve(res.as_dict().unwrap().get(b"Font").unwrap());
    assert!(fonts.as_dict().unwrap().contains(b"PCHelv") && fonts.as_dict().unwrap().contains(b"F1"));
    let shared = doc.get(printcraft_cos::ObjRef::new(6, 0));
    let shared_fonts = doc.resolve(shared.as_dict().unwrap().get(b"Font").unwrap());
    assert!(!shared_fonts.as_dict().unwrap().contains(b"PCHelv"), "the shared dictionary is untouched");
    assert_eq!(marks_present(&doc), [MarkKind::HeaderFooter]);
}

#[test]
fn replace_and_remove_restore_the_original_content() {
    let mut doc = fixture();
    let original = streams(&doc, 0);
    let mut hf = HeaderFooter::default();
    hf.text[1] = "First".into();
    add_header_footer(&mut doc, &[0], &hf, false, &cx()).unwrap();
    hf.text[1] = "Second".into();
    add_header_footer(&mut doc, &[0], &hf, true, &cx()).unwrap();
    let s = streams(&doc, 0);
    assert_eq!(s.iter().filter(|x| x.contains("/PCMark")).count(), 1, "replaced, not added");
    assert!(s.last().unwrap().contains("(Second)"));
    add_watermark(&mut doc, &[0], &Watermark { text: "DRAFT".into(), ..Watermark::default() }, false).unwrap();
    add_background(&mut doc, &[0], &Background { color: [1.0, 1.0, 0.9], opacity: 1.0, ..Background::default() }, false).unwrap();
    assert_eq!(marks_present(&doc).len(), 3);
    assert_eq!(remove_marks(&mut doc, &[0], MarkKind::HeaderFooter).unwrap(), 1);
    assert_eq!(remove_marks(&mut doc, &[0], MarkKind::Watermark).unwrap(), 1);
    let s = streams(&doc, 0);
    assert!(s[0].contains("/PCMark /Background"), "the background stays behind: {s:?}");
    assert_eq!(remove_marks(&mut doc, &[0], MarkKind::Background).unwrap(), 1);
    assert_eq!(streams(&reopen(&doc), 0), original, "back to exactly the original content");
}

#[test]
fn watermarks_rotate_fade_and_can_go_behind() {
    let mut doc = fixture();
    let wm = Watermark { text: "CONFIDENTIAL\nDo not copy".into(), opacity: 0.3, rotation: 45.0, behind: true, ..Watermark::default() };
    add_watermark(&mut doc, &[0], &wm, false).unwrap();
    let s = streams(&doc, 0);
    assert!(s[0].contains("/PCMark /Watermark"), "behind: first");
    assert!(s[0].contains("/PCGS30 gs") && s[0].contains("0.707 0.707 -0.707 0.707 300 400 cm"), "{}", s[0]);
    assert!(s[0].contains("(CONFIDENTIAL) Tj") && s[0].contains("(Do not copy) Tj"));
    assert_eq!(s.len(), 2, "no wrapper needed for content behind");
}

#[test]
fn invalid_requests_change_nothing() {
    let mut doc = fixture();
    assert!(matches!(add_header_footer(&mut doc, &[0], &HeaderFooter::default(), false, &cx()), Err(EditError::Invalid(_))));
    assert!(matches!(add_watermark(&mut doc, &[0], &Watermark::default(), false), Err(EditError::Invalid(_))));
    assert_eq!(
        add_background(&mut doc, &[9], &Background { color: [1.0; 3], opacity: 1.0, ..Background::default() }, false),
        Err(EditError::NoSuchPage(9))
    );
    assert!(!doc.is_modified());
}

#[test]
fn flattening_draws_appearances_into_the_page_and_removes_the_comments() {
    use printcraft_annot::{Meta, NewAnnotation, NoteIcon, Shape, Style, add_annotation, add_reply};
    let mut doc = fixture();
    let meta = Meta { date: None, id: "x".into() };
    let add = |doc: &mut Document, shape: Shape| {
        let style = Style::default_for(&shape);
        add_annotation(doc, &NewAnnotation { page: 0, shape, style, contents: "c".into(), author: "a".into() }, &meta).unwrap()
    };
    add(&mut doc, Shape::Rectangle { rect: [10.0, 10.0, 110.0, 60.0] });
    let note = add(&mut doc, Shape::Note { at: [200.0, 700.0], icon: NoteIcon::Comment });
    add_reply(&mut doc, 0, note, "reply", "b", &meta).unwrap();
    let before = streams(&doc, 0);
    let n = flatten(&mut doc, &[0], true, false).unwrap();
    assert_eq!(n, 2, "the rectangle and the note icon are drawn; the reply has nothing to draw");
    let doc = reopen(&doc);
    let p = &printcraft_model::pages(&doc)[0];
    assert!(!p.dict.contains(b"Annots"), "comments, pop-up and reply are gone");
    let s = streams(&doc, 0);
    assert_eq!(s.len(), before.len() + 3, "wrapped original + flattened content: {s:?}");
    let flat = s.last().unwrap();
    assert!(flat.contains("/PCFl0 Do") && flat.contains("/PCFl1 Do"), "{flat}");
    // The rectangle's appearance is drawn at its rectangle (bbox = rect, identity mapping).
    assert!(flat.contains("q 1 0 0 1 0 0 cm /PCFl0 Do Q"), "{flat}");
    // The note's 20×20 icon box is mapped onto its rect at (200, 680).
    assert!(flat.contains("1 0 0 1 200 680 cm /PCFl1 Do"), "{flat}");
    let res = doc.resolve(p.dict.get(b"Resources").unwrap());
    let xo = doc.resolve(res.as_dict().unwrap().get(b"XObject").unwrap());
    assert!(xo.as_dict().unwrap().contains(b"PCFl1"));
    assert!(marks_present(&doc).is_empty(), "flattened content is not a removable mark");
}

#[test]
fn added_text_and_images_are_page_content_that_stays_editable() {
    let mut doc = fixture();
    let text = AddedText { rect: [72.0, 600.0, 300.0, 700.0], text: "Approved by Ada\nSecond line".into(), size: 14.0, ..AddedText::default() };
    assert_eq!(add_content(&mut doc, 0, &Content::Text(text.clone())).unwrap(), 0);
    // An 1×1 gray image, placed on the rotated page.
    let mut d = Dict::new();
    d.set(b"Type".to_vec(), Object::name("XObject"));
    d.set(b"Subtype".to_vec(), Object::name("Image"));
    d.set(b"Width".to_vec(), Object::Int(1));
    d.set(b"Height".to_vec(), Object::Int(1));
    d.set(b"ColorSpace".to_vec(), Object::name("DeviceGray"));
    d.set(b"BitsPerComponent".to_vec(), Object::Int(8));
    let img = doc.add(Object::Stream(Stream::from_raw(d, vec![128])));
    add_content(&mut doc, 1, &Content::Image(AddedImage::new([10.0, 10.0, 110.0, 60.0], img))).unwrap();
    let doc2 = reopen(&doc);
    let all = list_added(&doc2);
    assert_eq!(all.len(), 2);
    let Content::Text(t) = &all[0].content else { panic!() };
    assert_eq!((t.text.as_str(), t.size), ("Approved by Ada\nSecond line", 14.0));
    assert_eq!(t.rect, [72.0, 700.0 - 2.0 * 14.0 * 1.2, 300.0, 700.0], "the box height follows the two lines");
    let page0 = streams(&doc2, 0).join("\n");
    assert!(page0.contains("(Approved by Ada) Tj") && page0.contains("(Second line) Tj") && page0.contains("/PCFHelvetica 14 Tf"), "{page0}");
    // The rotated page draws in display space: the view matrix comes first.
    let page1 = streams(&doc2, 1).join("\n");
    assert!(page1.contains("q 0 1 -1 0 600 0 cm") && page1.contains(&format!("/PCImg{} Do", img.num)), "{page1}");
    // Edit: move, restyle, retype; then delete.
    let mut doc = doc2;
    let moved = AddedText {
        rect: [100.0, 500.0, 300.0, 520.0],
        text: "Approved".into(),
        bold: true,
        family: Family::Times,
        align: Align::Right,
        color: [1.0, 0.0, 0.0],
        ..text
    };
    update_content(&mut doc, 0, 0, &Content::Text(moved)).unwrap();
    let page0 = streams(&doc, 0).join("\n");
    assert!(page0.contains("/PCFTimesBold 14 Tf 1 0 0 rg") && !page0.contains("Second line"), "{page0}");
    assert!(
        update_content(&mut doc, 1, 0, &Content::Text(AddedText { text: "x".into(), rect: [0.0, 0.0, 50.0, 10.0], ..AddedText::default() })).is_err(),
        "kinds don't change"
    );
    delete_content(&mut doc, 0, 0).unwrap();
    assert_eq!(list_added(&doc).len(), 1);
    assert!(!streams(&doc, 0).join("").contains("Approved"));
    assert!(add_content(&mut doc, 0, &Content::Text(AddedText { rect: [0.0, 0.0, 100.0, 10.0], ..AddedText::default() })).is_err(), "empty text");
    // Page marks ignore added items.
    assert!(marks_present(&doc).is_empty());
}

/// One page with Helvetica (WinAnsi) and a subset font that has only the glyphs it uses.
fn text_page(content: &str) -> Document {
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        // Subset: glyphs for a (97) and b (98) only.
        "<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Arial /FirstChar 97 /LastChar 99 /Widths [500 520 0] /Encoding /WinAnsiEncoding >>"
            .into(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    Document::open(Arc::new(out)).unwrap()
}

fn descriptor_text_page() -> Document {
    let content = "BT /F1 12 Tf 72 700 Td (ab) Tj ET";
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+NeutralFace /FirstChar 97 /LastChar 98 /Widths [500 500] /FontDescriptor 6 0 R /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /FontDescriptor /FontName /NeutralFace /Flags 262208 /ItalicAngle -12 /FontWeight 700 /Ascent 900 /Descent -250 >>".into(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    Document::open(Arc::new(out)).unwrap()
}

/// The Japanese fallback face comes from craft-fonts, an optional build input.
fn without_craft_fonts(test: &str) -> bool {
    if printcraft_fonts::document_japanese_font().is_some() {
        return false;
    }
    eprintln!("skipping {test}: built without craft-fonts (set CRAFT_FONTS_DIR to run it)");
    true
}

#[test]
fn japanese_line_uses_unicode_type3_fallback() {
    if without_craft_fonts("japanese_line_uses_unicode_type3_fallback") {
        return;
    }
    let mut doc = text_page("BT /F2 12 Tf 72 700 Td (ab) Tj ET");
    let replacement = "25362738こんにちは、お元気ですか？ 2.3444";
    let result = text::replace_line(&mut doc, 0, 0, replacement).unwrap();
    assert_eq!(result.substituted.as_deref(), Some("Shippori Mincho Type3"));
    let bytes = page_content_bytes(&doc, 0);
    let content = String::from_utf8_lossy(&bytes);
    assert!(content.contains("/PCJp"), "{content}");
    let reopened = reopen(&doc);
    assert_eq!(text::text_lines(&reopened, 0).unwrap()[0].text, replacement);
}

#[test]
fn japanese_paragraph_uses_unicode_type3_fallback() {
    if without_craft_fonts("japanese_paragraph_uses_unicode_type3_fallback") {
        return;
    }
    let mut doc = text_page("BT /F2 12 Tf 72 700 Td (ab) Tj ET");
    let replacement = "こんにちは、お元気ですか？ 2.3444 日本語の文章";
    text::replace_block(&mut doc, 0, 0, replacement).unwrap();
    let reopened = reopen(&doc);
    assert_eq!(text::text_blocks(&reopened, 0).unwrap()[0].text, replacement);
}

/// Without craft-fonts there is no Japanese face: editing in Japanese is a clear error that
/// leaves the page untouched, never a panic; Latin edits work as before.
#[test]
fn japanese_edit_without_craft_fonts_is_a_clear_error() {
    let mut doc = text_page("BT /F2 12 Tf 72 700 Td (ab) Tj ET");
    let before = page_content_bytes(&doc, 0);
    let line = text::replace_line(&mut doc, 0, 0, "日本語の文字");
    let block = text::replace_block(&mut doc, 0, 0, "日本語の文字");
    if printcraft_fonts::document_japanese_font().is_some() {
        eprintln!("built with craft-fonts: the Japanese edits succeed (checked by the tests above)");
        assert!(line.is_ok() && block.is_ok());
        return;
    }
    for result in [line.map(|_| ()), block.map(|_| ())] {
        let Err(EditError::Invalid(msg)) = result else { panic!("expected a clear error, got {result:?}") };
        assert!(msg.contains("Japanese fallback font") && msg.contains("CRAFT_FONTS_DIR"), "{msg}");
    }
    assert_eq!(page_content_bytes(&doc, 0), before);
    let latin = text::replace_line(&mut doc, 0, 0, "Hello").unwrap();
    assert_eq!(text::text_lines(&reopen(&doc), 0).unwrap()[0].text, "Hello", "{latin:?}");
}
#[test]
fn font_descriptor_style_is_exposed_even_with_a_neutral_name() {
    let doc = descriptor_text_page();
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].bold && lines[0].italic, "{:?}", lines[0]);
    let blocks = text::text_blocks(&doc, 0).unwrap();
    assert!(blocks[0].bold && blocks[0].italic, "{:?}", blocks[0]);
}

#[test]
fn mixed_font_runs_are_not_merged_into_one_source_style() {
    let doc = text_page("BT /F1 12 Tf 72 700 Td (Regular) Tj /F2 12 Tf 150 700 Td (Subset) Tj ET");
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(lines.len(), 2);
    assert_ne!(lines[0].font, lines[1].font);
}

#[test]
fn missing_glyphs_preserve_source_style_in_fallback() {
    let mut doc = descriptor_text_page();
    let result = text::replace_line(&mut doc, 0, 0, "Styled €").unwrap();
    assert_eq!(result.substituted.as_deref(), Some("Helvetica-BoldOblique"));
    let bytes = page_content_bytes(&doc, 0);
    let content = String::from_utf8_lossy(&bytes);
    assert!(content.contains("/PCEdHelveticaBoldOblique 12 Tf"), "{content}");
}

#[test]
fn text_lines_are_found_and_replaced_in_place() {
    let mut doc =
        text_page("BT /F1 12 Tf 72 700 Td (Hello) Tj 40 0 Td [(wor) -20 (ld)] TJ 0 -20 Td (Second line) Tj ET BT /F2 10 Tf 72 600 Td (ab) Tj ET");
    let lines = text::text_lines(&doc, 0).unwrap();
    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["Hello world", "Second line", "ab"]);
    assert!((lines[0].rect[0] - 72.0).abs() < 0.01 && lines[0].rect[1] < 700.0 && lines[0].rect[3] > 700.0, "{:?}", lines[0].rect);
    assert!((lines[0].size - 12.0).abs() < 1e-9 && lines[0].base_font == "Helvetica");
    let second = lines[1].rect;
    let r = text::replace_line(&mut doc, 0, 0, "Goodbye, café").unwrap();
    assert_eq!(r.substituted, None);
    let doc = reopen(&doc);
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(lines[0].text, "Goodbye, café");
    assert_eq!(lines[1].text, "Second line");
    assert_eq!(lines[1].rect, second);
}

#[test]
fn missing_glyphs_substitute_helvetica_and_impossible_text_is_refused() {
    let mut doc = text_page("BT /F2 10 Tf 72 600 Td (ab) Tj ET");
    // "c" has no glyph in the subset: Helvetica takes over for this line.
    let r = text::replace_line(&mut doc, 0, 0, "abc").unwrap();
    assert_eq!(r.substituted.as_deref(), Some("Helvetica"));
    let doc2 = reopen(&doc);
    let lines = text::text_lines(&doc2, 0).unwrap();
    assert_eq!((lines[0].text.as_str(), lines[0].base_font.as_str()), ("abc", "Helvetica"));
    // Neither font can show Greek.
    let mut doc = text_page("BT /F1 12 Tf 72 700 Td (Hello) Tj ET");
    assert!(text::replace_line(&mut doc, 0, 0, "Ωmega").is_err());
    assert!(text::replace_line(&mut doc, 0, 5, "x").is_err(), "no such line");
}

#[test]
fn paragraphs_are_found_and_rewrapped() {
    let mut doc = text_page(
        "BT 0 0 1 rg /F1 10 Tf 12 TL 72 700 Td (The quick brown fox) Tj T* (jumps over the) Tj T* (lazy dog.) Tj ET \
         BT /F1 10 Tf 72 600 Td (Next paragraph) Tj ET",
    );
    let blocks = text::text_blocks(&doc, 0).unwrap();
    let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, ["The quick brown fox jumps over the lazy dog.", "Next paragraph"]);
    assert_eq!(blocks[0].lines, [0, 1, 2]);
    let width = blocks[0].rect[2] - blocks[0].rect[0];
    let next = blocks[1].rect;
    let long = "PrintCraft rewraps a paragraph to its own width when its text changes, keeping the font, size, colour and line spacing.";
    assert_eq!(text::replace_block(&mut doc, 0, 0, long).unwrap().substituted, None);
    let doc = reopen(&doc);
    let lines = text::text_lines(&doc, 0).unwrap();
    let blocks = text::text_blocks(&doc, 0).unwrap();
    assert_eq!(blocks[0].text, long);
    assert!(blocks[0].lines.len() > 3, "more lines: {}", blocks[0].lines.len());
    for i in &blocks[0].lines {
        assert!(lines[*i].rect[2] - lines[*i].rect[0] <= width + 1.0, "line {} fits", lines[*i].text);
    }
    // Same left edge, same spacing (12 pt).
    assert!((lines[0].rect[0] - 72.0).abs() < 0.01);
    let spacing = lines[blocks[0].lines[0]].origin_baseline() - lines[blocks[0].lines[1]].origin_baseline();
    assert!((spacing - 12.0).abs() < 0.01, "{spacing}");
    // The next paragraph is untouched, and the new text is blue like the old.
    assert_eq!(blocks[1].text, "Next paragraph");
    assert_eq!(blocks[1].rect, next);
    let content = String::from_utf8_lossy(&page_content_bytes(&doc, 0)).into_owned();
    assert!(content.contains("0 0 1 rg"), "{content}");
    // Shorter text: fewer lines.
    let mut doc = doc;
    text::replace_block(&mut doc, 0, 0, "Short.").unwrap();
    let blocks = text::text_blocks(&doc, 0).unwrap();
    assert_eq!((blocks[0].text.as_str(), blocks[0].lines.len()), ("Short.", 1));
}

#[test]
fn chinese_cid_text_survives_reopening_and_a_second_edit() {
    if !printcraft_fonts::CRAFT_FONTS.iter().any(|f| f.covers("Hans")) {
        return;
    }
    let mut doc = text_page("BT /F1 12 Tf 72 700 Td (Original) Tj ET BT /F1 12 Tf 72 550 Td (Keep me) Tj ET");
    let original_next = text::text_lines(&doc, 0).unwrap()[1].clone();
    let first = "简体中文字体与字号可以修改 PDF 2026";
    let result = text::replace_block(&mut doc, 0, 0, first).unwrap();
    assert!(result.substituted.unwrap().contains("IBMPlexSansSC"));
    let mut doc = reopen(&doc);
    assert_eq!(text::text_lines(&doc, 0).unwrap()[0].text, first);
    assert_eq!(text::text_lines(&doc, 0).unwrap()[1].rect, original_next.rect);
    assert_eq!(text::text_lines(&doc, 0).unwrap()[1].text, original_next.text);
    let second = "重新编辑并保存：新增汉字测试";
    text::rewrite_block(
        &mut doc,
        0,
        0,
        Some(second),
        &text::BlockStyle {
            family: Some((crate::added::Family::Helvetica, true, true)),
            size: Some(18.0),
            color: Some([0.8, 0.0, 0.1]),
            ..Default::default()
        },
    )
    .unwrap();
    let doc = reopen(&doc);
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(lines[0].text, second);
    assert!(lines[0].bold && lines[0].italic && (lines[0].size - 18.0).abs() < 0.01);
    assert_eq!(lines[0].color, [0.8, 0.0, 0.1]);
    assert_eq!(lines[1].rect, original_next.rect);
    assert_eq!(lines[1].text, original_next.text);
}

#[test]
fn chinese_over_240_glyphs_wraps_without_losing_characters() {
    if !printcraft_fonts::CRAFT_FONTS.iter().any(|f| f.covers("Hans")) {
        return;
    }
    let replacement: String = (0x4e00..0x4e00 + 300).filter_map(char::from_u32).collect();
    let mut doc = text_page("BT /F1 12 Tf 72 700 Td (Original) Tj ET");
    text::rewrite_block(&mut doc, 0, 0, Some(&replacement), &text::BlockStyle { width: Some(120.0), ..Default::default() }).unwrap();
    let doc = reopen(&doc);
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<String>(), replacement);
    assert!(lines.len() > 20);
    assert!(lines.iter().all(|l| l.rect[2] - l.rect[0] <= 121.0));
}

#[test]
fn explicit_line_breaks_are_preserved() {
    let mut doc = text_page("BT /F1 12 Tf 72 700 Td (Original) Tj ET");
    text::rewrite_block(&mut doc, 0, 0, Some("first\nsecond\nthird"), &text::BlockStyle { width: Some(80.0), ..Default::default() }).unwrap();
    let doc = reopen(&doc);
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(lines[0].text, "first");
    assert_eq!(lines[1].text, "second");
    assert_eq!(lines[2].text, "third");
    assert!(lines.iter().all(|l| l.rect[2] - l.rect[0] <= 81.0));
}

fn page_content_bytes(doc: &Document, page: usize) -> Vec<u8> {
    let p = printcraft_model::pages(doc).swap_remove(page);
    let c = p.dict.get(b"Contents").unwrap();
    match &*doc.resolve(c) {
        printcraft_cos::Object::Stream(s) => s.decoded().unwrap(),
        printcraft_cos::Object::Array(a) => a
            .iter()
            .flat_map(|x| match &*doc.resolve(x) {
                printcraft_cos::Object::Stream(s) => s.decoded().unwrap(),
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A page drawing image XObject /Im0 at 100,100 size 200 × 100 (pixels 4 × 2).
fn image_page() -> Document {
    let content = "q 200 0 0 100 100 100 cm /Im0 Do Q BT /F1 12 Tf 72 700 Td (Caption) Tj ET";
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> >> >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /XObject /Subtype /Image /Width 4 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 8 >>\nstream\n\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\nendstream".into(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    Document::open(Arc::new(out)).unwrap()
}

fn close(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn page_images_move_turn_replace_and_delete() {
    let mut doc = image_page();
    let imgs = images::page_images(&doc, 0).unwrap();
    assert_eq!(imgs.len(), 1);
    assert!(close(imgs[0].rect, [100.0, 100.0, 300.0, 200.0]), "{:?}", imgs[0].rect);
    assert_eq!((imgs[0].width, imgs[0].height, imgs[0].name.as_str()), (4, 2, "Im0"));
    // Move and resize.
    let t = images::rect_to_rect(imgs[0].rect, [50.0, 400.0, 150.0, 450.0]);
    images::change_image(&mut doc, 0, 0, &images::ImageChange::Transform(t)).unwrap();
    let doc = reopen(&doc);
    let r = images::page_images(&doc, 0).unwrap()[0].rect;
    assert!(close(r, [50.0, 400.0, 150.0, 450.0]), "{r:?}");
    // A quarter turn about the centre: 100 × 50 becomes 50 × 100 around (100, 425).
    let mut doc = doc;
    let t = images::turn_about_centre(r, 1, false, false);
    images::change_image(&mut doc, 0, 0, &images::ImageChange::Transform(t)).unwrap();
    let r = images::page_images(&doc, 0).unwrap()[0].rect;
    assert!(close(r, [75.0, 375.0, 125.0, 475.0]), "{r:?}");
    // The text after it is untouched.
    assert_eq!(text::text_lines(&doc, 0).unwrap()[0].text, "Caption");
    // Replace with another image object, in the same place.
    let mut d = printcraft_cos::Dict::new();
    for (k, v) in [
        (&b"Type"[..], printcraft_cos::Object::name("XObject")),
        (b"Subtype", printcraft_cos::Object::name("Image")),
        (b"Width", printcraft_cos::Object::Int(1)),
        (b"Height", printcraft_cos::Object::Int(1)),
    ] {
        d.set(k.to_vec(), v);
    }
    let other = doc.add(printcraft_cos::Object::Stream(printcraft_cos::Stream::from_raw(d, vec![0])));
    images::change_image(&mut doc, 0, 0, &images::ImageChange::Replace(other)).unwrap();
    let imgs = images::page_images(&doc, 0).unwrap();
    assert_eq!((imgs[0].object, imgs[0].width), (Some(other), 1));
    assert!(close(imgs[0].rect, r));
    // Delete.
    images::change_image(&mut doc, 0, 0, &images::ImageChange::Delete).unwrap();
    assert!(images::page_images(&doc, 0).unwrap().is_empty());
    assert!(images::change_image(&mut doc, 0, 0, &images::ImageChange::Delete).is_err());
}

#[test]
fn paragraphs_take_new_formatting() {
    let mut doc = text_page("BT /F1 10 Tf 12 TL 100 700 Td (One two three four five six seven) Tj T* (eight nine ten eleven twelve) Tj ET");
    let before = text::text_blocks(&doc, 0).unwrap()[0].clone();
    let style = text::BlockStyle {
        family: Some((added::Family::Times, true, false)),
        size: Some(12.0),
        color: Some([1.0, 0.0, 0.0]),
        align: Some(added::Align::Center),
        ..Default::default()
    };
    text::rewrite_block(&mut doc, 0, 0, None, &style).unwrap();
    let doc = reopen(&doc);
    let blocks = text::text_blocks(&doc, 0).unwrap();
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(blocks[0].text, before.text, "same words");
    assert_eq!((blocks[0].base_font.as_str(), blocks[0].size), ("Times-Bold", 12.0));
    // Centred in the paragraph's width.
    let mid = (before.rect[0] + before.rect[2]) / 2.0;
    for i in &blocks[0].lines {
        let r = lines[*i].rect;
        assert!(((r[0] + r[2]) / 2.0 - mid).abs() < 2.0, "line {:?} centred on {mid}", lines[*i].text);
    }
    assert!(String::from_utf8_lossy(&page_content_bytes(&doc, 0)).contains("1 0 0 rg"));
    // Right alignment keeps lines flush with the right edge.
    let mut doc = text_page("BT /F1 10 Tf 12 TL 100 700 Td (One two three four five six seven) Tj T* (eight nine ten eleven twelve) Tj ET");
    let right = text::text_blocks(&doc, 0).unwrap()[0].rect[2];
    text::rewrite_block(&mut doc, 0, 0, None, &text::BlockStyle { align: Some(added::Align::Right), ..Default::default() }).unwrap();
    let lines = text::text_lines(&doc, 0).unwrap();
    for l in &lines {
        assert!((l.rect[2] - right).abs() < 1.0, "{:?} ends at {} not {right}", l.text, l.rect[2]);
    }
}

#[test]
fn a_new_paragraph_colour_does_not_spill_into_the_text_after_it() {
    let mut doc = text_page("BT /F1 10 Tf 72 700 Td (First paragraph) Tj 0 -40 Td (Second paragraph) Tj ET");
    let style = text::BlockStyle { color: Some([1.0, 0.0, 0.0]), ..Default::default() };
    text::rewrite_block(&mut doc, 0, 0, None, &style).unwrap();
    let doc = reopen(&doc);
    // The fill colour in force where the second paragraph is shown (q/Q nest it).
    let ops = printcraft_content::parse(&page_content_bytes(&doc, 0)).ops;
    let (mut fill, mut stack) = (String::from("0 g"), Vec::new());
    for op in &ops {
        match op.op.as_slice() {
            b"q" => stack.push(fill.clone()),
            b"Q" => fill = stack.pop().unwrap_or_default(),
            b"g" | b"rg" | b"k" => fill = String::from_utf8_lossy(&printcraft_content::serialize_ops(std::slice::from_ref(op))).trim().to_string(),
            b"Tj" if op.operands.first().and_then(|o| o.as_string()).is_some_and(|s| s.to_text() == "Second paragraph") => break,
            _ => {}
        }
    }
    assert_eq!(fill, "0 g", "{ops:?}");
    assert_eq!(text::text_blocks(&doc, 0).unwrap()[1].text, "Second paragraph");
}

#[test]
fn a_rewritten_paragraph_keeps_its_place_in_a_shared_text_object() {
    // Three paragraphs in one BT … ET: rewriting the second keeps the order (paragraph numbers
    // and reading order) and every paragraph's position.
    let src = "BT /F1 10 Tf 72 700 Td (First paragraph) Tj 0 -40 Td (Second paragraph) Tj 0 -40 Td (Third paragraph) Tj ET";
    let mut doc = text_page(src);
    let before = text::text_blocks(&doc, 0).unwrap();
    text::replace_block(&mut doc, 0, 1, "Edited second").unwrap();
    let doc = reopen(&doc);
    let after = text::text_blocks(&doc, 0).unwrap();
    let texts: Vec<&str> = after.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, ["First paragraph", "Edited second", "Third paragraph"]);
    assert_eq!(after[0].rect, before[0].rect);
    assert_eq!(after[2].rect, before[2].rect);
    assert!((after[1].rect[0] - before[1].rect[0]).abs() < 0.01 && (after[1].rect[1] - before[1].rect[1]).abs() < 0.01, "{:?}", after[1].rect);
}

#[test]
fn recolouring_a_paragraph_in_a_shared_text_object_keeps_order_and_nesting() {
    // Splitting the text object (to keep the order) and q … Q (to keep the colour in) together:
    // q/Q stay outside text objects, and the paragraph after it keeps the original colour.
    let src = "BT /F1 10 Tf 72 700 Td (First paragraph) Tj 0 -40 Td (Second paragraph) Tj 0 -40 Td (Third paragraph) Tj ET";
    let mut doc = text_page(src);
    let style = text::BlockStyle { color: Some([1.0, 0.0, 0.0]), ..Default::default() };
    text::rewrite_block(&mut doc, 0, 1, None, &style).unwrap();
    let doc = reopen(&doc);
    let texts: Vec<String> = text::text_blocks(&doc, 0).unwrap().into_iter().map(|b| b.text).collect();
    assert_eq!(texts, ["First paragraph", "Second paragraph", "Third paragraph"]);
    let ops = printcraft_content::parse(&page_content_bytes(&doc, 0)).ops;
    let (mut in_text, mut depth, mut red) = (false, 0usize, Vec::new());
    for op in &ops {
        match op.op.as_slice() {
            b"BT" => in_text = true,
            b"ET" => in_text = false,
            b"q" | b"Q" => {
                assert!(!in_text, "q/Q inside a text object: {ops:?}");
                depth = if op.is("q") { depth + 1 } else { depth.saturating_sub(1) };
            }
            b"rg" => red.push(depth),
            b"Tj" if op.operands.first().and_then(|o| o.as_string()).is_some_and(|s| s.to_text() == "Third paragraph") => {
                assert!(red.iter().all(|d| *d > depth), "red still in force for the third paragraph: {ops:?}");
            }
            _ => {}
        }
    }
    assert!(!red.is_empty(), "{ops:?}");
}

#[test]
fn justify_underline_and_spacing() {
    let src = "BT /F1 10 Tf 12 TL 100 700 Td (One two three four five six seven) Tj T* (eight nine ten eleven twelve) Tj T* (end) Tj ET";
    // Justified: every line but the last reaches the right edge.
    let mut doc = text_page(src);
    let right = text::text_blocks(&doc, 0).unwrap()[0].rect[2];
    text::rewrite_block(&mut doc, 0, 0, None, &text::BlockStyle { align: Some(added::Align::Justify), ..Default::default() }).unwrap();
    let lines = text::text_lines(&doc, 0).unwrap();
    for l in &lines[..lines.len() - 1] {
        assert!((l.rect[2] - right).abs() < 1.0, "{:?} ends at {} not {right}", l.text, l.rect[2]);
    }
    // Double line spacing, underlined.
    let mut doc = text_page(src);
    let style = text::BlockStyle { line_spacing: Some(2.0), underline: Some(true), ..Default::default() };
    text::rewrite_block(&mut doc, 0, 0, None, &style).unwrap();
    let lines = text::text_lines(&doc, 0).unwrap();
    assert!((lines[0].origin_baseline() - lines[1].origin_baseline() - 20.0).abs() < 0.01);
    let content = String::from_utf8_lossy(&page_content_bytes(&doc, 0)).into_owned();
    assert!(content.contains(" l\nS\n") || content.contains(" l S"), "{content}");
    // Character spacing widens, horizontal scale narrows.
    let mut doc = text_page("BT /F1 10 Tf 100 700 Td (Wide text) Tj ET");
    let w0 = text::text_lines(&doc, 0).unwrap()[0].rect;
    text::rewrite_block(&mut doc, 0, 0, None, &text::BlockStyle { char_spacing: Some(2.0), ..Default::default() }).unwrap();
    let w1 = text::text_lines(&doc, 0).unwrap()[0].rect;
    assert!((w1[2] - w1[0]) - (w0[2] - w0[0]) > 15.0, "{w0:?} → {w1:?}");
    let mut doc = text_page("BT /F1 10 Tf 100 700 Td (Wide text) Tj ET");
    text::rewrite_block(&mut doc, 0, 0, None, &text::BlockStyle { scale: Some(50.0), ..Default::default() }).unwrap();
    let w2 = text::text_lines(&doc, 0).unwrap()[0].rect;
    assert!(((w2[2] - w2[0]) - (w0[2] - w0[0]) / 2.0).abs() < 1.0, "{w0:?} → {w2:?}");
}

#[test]
fn a_line_drawn_twice_is_replaced_everywhere() {
    // Fake bold: a line drawn twice at the same spot (each copy its own text object). The two
    // copies group apart, but editing one replaces both — a surviving copy would show the old
    // text under the new.
    let mut doc = text_page("BT /F1 12 Tf 72 700 Td (Hello) Tj ET BT /F1 12 Tf 72 700 Td (Hello) Tj ET");
    assert_eq!(text::text_blocks(&doc, 0).unwrap().len(), 2, "the copies group apart");
    text::replace_block(&mut doc, 0, 0, "Hello world").unwrap();
    let doc = reopen(&doc);
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["Hello world"], "one line, replaced");
    let content = String::from_utf8_lossy(&page_content_bytes(&doc, 0)).into_owned();
    assert_eq!(content.matches("(Hello").count(), 1, "the second copy's operators are gone: {content}");
    // Copies in one text object (the matrix re-issued at the same spot) go too.
    let mut doc = text_page("BT /F1 12 Tf 1 0 0 1 72 700 Tm (Hi) Tj 1 0 0 1 72.3 700 Tm (Hi) Tj ET");
    text::replace_line(&mut doc, 0, 0, "Hey").unwrap();
    let doc = reopen(&doc);
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["Hey"], "one line, replaced");
    // A neighbouring line is not coincident and stays (40 pt spacing groups apart at 12 pt).
    let mut doc = text_page("BT /F1 12 Tf 72 700 Td (Hello) Tj 0 -40 Td (world) Tj ET");
    text::replace_block(&mut doc, 0, 0, "Hello there").unwrap();
    let doc = reopen(&doc);
    let lines = text::text_lines(&doc, 0).unwrap();
    assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["Hello there", "world"]);
}

#[test]
fn paragraphs_move_and_rewrap_to_a_new_width() {
    let para = "BT /F1 10 Tf 12 TL 100 700 Td (One two three four five six seven) Tj T* (eight nine ten eleven twelve) Tj ET";
    let close = |a: f64, b: f64| (a - b).abs() < 0.5;
    // Moved 50 right and 200 down, same words and wrapping.
    let mut doc = text_page(para);
    let before = text::text_blocks(&doc, 0).unwrap()[0].clone();
    text::rewrite_block(&mut doc, 0, 0, None, &text::BlockStyle { offset: Some([50.0, -200.0]), ..Default::default() }).unwrap();
    let doc = reopen(&doc);
    let after = text::text_blocks(&doc, 0).unwrap()[0].clone();
    assert_eq!((after.text.as_str(), after.lines.len()), (before.text.as_str(), before.lines.len()));
    assert!(close(after.rect[0], before.rect[0] + 50.0) && close(after.rect[1], before.rect[1] - 200.0), "{:?} → {:?}", before.rect, after.rect);
    // Drawn at half scale: the move is still in page space.
    let mut doc = text_page(&format!("q 0.5 0 0 0.5 0 0 cm {} Q", para.replace("100 700 Td", "200 1400 Td")));
    let before = text::text_blocks(&doc, 0).unwrap()[0].clone();
    text::rewrite_block(&mut doc, 0, 0, None, &text::BlockStyle { offset: Some([30.0, 40.0]), ..Default::default() }).unwrap();
    let after = text::text_blocks(&reopen(&doc), 0).unwrap()[0].clone();
    assert!(close(after.rect[0], before.rect[0] + 30.0) && close(after.rect[1], before.rect[1] + 40.0), "{:?} → {:?}", before.rect, after.rect);
    // A narrower box rewraps into more lines that fit it; a wider one into fewer.
    let mut doc = text_page(para);
    let left = text::text_blocks(&doc, 0).unwrap()[0].rect[0];
    text::rewrite_block(&mut doc, 0, 0, None, &text::BlockStyle { width: Some(80.0), ..Default::default() }).unwrap();
    let narrow = text::text_blocks(&reopen(&doc), 0).unwrap()[0].clone();
    assert!(narrow.lines.len() > 2, "{narrow:?}");
    assert!(narrow.rect[2] <= left + 80.5, "{narrow:?}");
    let mut doc = text_page(para);
    text::rewrite_block(&mut doc, 0, 0, None, &text::BlockStyle { width: Some(400.0), ..Default::default() }).unwrap();
    assert_eq!(text::text_blocks(&reopen(&doc), 0).unwrap()[0].lines.len(), 1);
    // Untrusted numbers (the automation tools pass them through) are refused, not drawn.
    let mut doc = text_page(para);
    for style in [
        text::BlockStyle { offset: Some([f64::NAN, 0.0]), ..Default::default() },
        text::BlockStyle { offset: Some([0.0, f64::INFINITY]), ..Default::default() },
        text::BlockStyle { width: Some(f64::NAN), ..Default::default() },
    ] {
        assert!(text::rewrite_block(&mut doc, 0, 0, None, &style).is_err(), "{style:?}");
    }
    // A zero width is clamped to one character's width rather than looping or vanishing.
    text::rewrite_block(&mut doc, 0, 0, None, &text::BlockStyle { width: Some(0.0), ..Default::default() }).unwrap();
    assert_eq!(text::text_blocks(&reopen(&doc), 0).unwrap().iter().map(|b| b.text.split_whitespace().count()).sum::<usize>(), 12);
}
