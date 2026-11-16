pub const fn draw(g: &fx_ground::Ground) -> u32 {
    fx_ground::tile(g) + fx_ids::tile_of(&g.at)
}
