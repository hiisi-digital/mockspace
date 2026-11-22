pub struct Ground {
    pub at: fx_ids::Id,
}

pub const fn tile(g: &Ground) -> u32 {
    fx_ids::tile_of(&g.at)
}
