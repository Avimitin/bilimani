use bilimani_iidx::{
    aliases::{AliasError, parse},
    catalog::Catalog,
    game::Song,
};
fn songs() -> Vec<Song> {
    [
        (1, "AA"),
        (2, "Title, with comma"),
        (3, "Shared"),
        (4, "Shared"),
    ]
    .into_iter()
    .map(|(id, title)| Song {
        id,
        title: title.into(),
        search_terms: vec![],
        charts: vec![],
    })
    .collect()
}
#[test]
fn csv_supports_bom_unicode_quoted_titles_and_atomic_reload() {
    let mut catalog = Catalog::new(songs()).unwrap();
    assert_eq!(
        catalog
            .reload_aliases("\u{feff}alias,target\n别名,1\n逗号,\"Title, with comma\"\n".as_bytes())
            .unwrap(),
        2
    );
    catalog.set_search_index(&[(1, "native term".into())]);
    assert_eq!(catalog.search("别名", 10)[0].id, 1);
    assert_eq!(catalog.search("逗号", 10)[0].id, 2);
    assert!(matches!(
        catalog.reload_aliases(b"alias,target\nnew,1\nbad,999\n"),
        Err(AliasError::Missing { line: 3, .. })
    ));
    assert_eq!(catalog.aliases_count(), 2);
    assert_eq!(catalog.search("别名", 10)[0].id, 1);
    assert!(catalog.search("new", 10).is_empty());
    catalog.reload_aliases(b"alias,target\n").unwrap();
    assert!(catalog.search("别名", 10).is_empty());
    assert_eq!(catalog.search("native", 10)[0].id, 1);
}
#[test]
fn csv_rejects_duplicates_ambiguous_titles_and_malformed_rows() {
    let songs = songs();
    assert!(matches!(
        parse("alias,target\nＡＡ,1\naa,2".as_bytes(), &songs),
        Err(AliasError::Duplicate { line: 3, .. })
    ));
    assert!(matches!(
        parse(b"alias,target\nx,Shared", &songs),
        Err(AliasError::Ambiguous { line: 2, .. })
    ));
    assert!(matches!(
        parse(b"target,alias\n1,x", &songs),
        Err(AliasError::Header)
    ));
    assert!(matches!(
        parse(b"alias,target\nx,1,2", &songs),
        Err(AliasError::Csv(_))
    ));
}
