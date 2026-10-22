//! Optional read-only asset integration; never embeds ROM geometry/pose fixtures.
#[test]
#[ignore = "needs the US revision 0 ROM via SM64_ROM"]
fn local_mario_idle_display_lists() {
    let Some(path) = std::env::var_os("SM64_ROM") else {
        eprintln!("skipped: set SM64_ROM to a user-supplied US revision 0 ROM");
        return;
    };
    let rom = sm64_rom::Rom::open(path).unwrap();
    let mario = sm64_level::actors::load_mario_idle(&rom).unwrap();
    let mut triangles = 0;
    let mut textures = 0;
    let mut commands = mario.geometry.commands_inspected;
    eprintln!(
        "Mario: {} geo draws, {} geo commands",
        mario.geometry.draws.len(),
        commands
    );
    let mut stream = sm64_gfx::DisplayListStream::new(&mario.segments);
    let mut red = 0;
    let mut blue = 0;
    let mut white = 0;
    let mut missing_roots = 0;
    for (root_index, draw) in mario.geometry.draws.iter().enumerate() {
        let decoded = stream
            .decode_next(draw.display_list, 1_000_000usize.saturating_sub(commands))
            .unwrap_or_else(|e| panic!("DL 0x{:08x}: {e}", draw.display_list));
        let missing = decoded
            .diagnostics
            .iter()
            .filter(|s| s.contains("Missing inherited"))
            .count();
        missing_roots += usize::from(missing > 0);
        eprintln!(
            "root {root_index} DL 0x{:08x}: {} commands, {} batches, {} textures, {missing} missing-light notices",
            draw.display_list,
            decoded.commands_inspected,
            decoded.batches.len(),
            decoded.textures.len()
        );
        for v in decoded.batches.iter().flat_map(|b| &b.vertices) {
            let [r, g, b, _] = v.color;
            red += usize::from(r > 0.1 && r > g * 2. && r > b * 2.);
            blue += usize::from(b > 0.1 && b > r * 2. && b > g * 2.);
            white += usize::from(r > 0.5 && (r - g).abs() < 0.03 && (r - b).abs() < 0.03);
        }
        commands += decoded.commands_inspected;
        triangles += decoded
            .batches
            .iter()
            .map(|b| b.vertices.len() / 3)
            .sum::<usize>();
        textures += decoded.textures.len();
    }
    assert!(mario.geometry.draws.len() > 10);
    assert!(triangles > 500);
    assert!(textures > 0);
    assert!(red > 0, "expected red shaded vertices");
    assert!(blue > 0, "expected blue shaded vertices");
    assert!(white > 0, "expected neutral bright glove vertices");
    eprintln!(
        "Shading: {red} red, {blue} blue, {white} white vertices; {missing_roots} roots missing inherited light state"
    );
    eprintln!(
        "Mario totals: {triangles} triangles, {textures} per-draw textures, {commands} commands"
    );
}
