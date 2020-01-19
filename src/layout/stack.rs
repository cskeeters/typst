use smallvec::smallvec;
use crate::size::ValueBox;
use super::*;


/// The stack layouter stack boxes onto each other along the secondary layouting
/// axis.
#[derive(Debug, Clone)]
pub struct StackLayouter {
    /// The context for layouting.
    ctx: StackContext,
    /// The output layouts.
    layouts: MultiLayout,
    /// The currently active layout space.
    space: Space,
}

/// The context for stack layouting.
#[derive(Debug, Clone)]
pub struct StackContext {
    /// The spaces to layout in.
    pub spaces: LayoutSpaces,
    /// The initial layouting axes, which can be updated by the
    /// [`StackLayouter::set_axes`] method.
    pub axes: LayoutAxes,
    /// Which alignment to set on the resulting layout. This affects how it will
    /// be positioned in a parent box.
    pub alignment: LayoutAlignment,
    /// Whether to have repeated spaces or to use only the first and only once.
    pub repeat: bool,
    /// Whether to output a command which renders a debugging box showing the
    /// extent of the layout.
    pub debug: bool,
}

/// A layout space composed of subspaces which can have different axes and
/// alignments.
#[derive(Debug, Clone)]
struct Space {
    /// The index of this space in the list of spaces.
    index: usize,
    /// Whether to add the layout for this space even if it would be empty.
    hard: bool,
    /// The so-far accumulated layouts.
    layouts: Vec<(LayoutAxes, Layout)>,
    /// The specialized size of this space.
    size: Size2D,
    /// The specialized remaining space.
    usable: Size2D,
    /// The specialized extra-needed dimensions to affect the size at all.
    extra: Size2D,
    /// The rulers of a space dictate which alignments for new boxes are still
    /// allowed and which require a new space to be started.
    rulers: ValueBox<Alignment>,
    /// The last added spacing if the last added thing was spacing.
    last_spacing: LastSpacing,
}

impl StackLayouter {
    /// Create a new stack layouter.
    pub fn new(ctx: StackContext) -> StackLayouter {
        let space = ctx.spaces[0];
        StackLayouter {
            ctx,
            layouts: MultiLayout::new(),
            space: Space::new(0, true, space.usable()),
        }
    }

    /// Add a layout to the stack.
    pub fn add(&mut self, layout: Layout) {
        // If the alignment cannot be fitted in this space, finish it.
        // TODO: Issue warning for non-fitting alignment in
        //       non-repeating context.
        if !self.update_rulers(layout.alignment) && self.ctx.repeat {
            self.finish_space(true);
        }

        // Now, we add a possibly cached soft space. If the secondary alignment
        // changed before, a possibly cached space would have already been
        // discarded.
        if let LastSpacing::Soft(spacing, _) = self.space.last_spacing {
            self.add_spacing(spacing, SpacingKind::Hard);
        }

        // TODO: Issue warning about overflow if there is overflow.
        if !self.space.usable.fits(layout.dimensions) && self.ctx.repeat {
            self.skip_to_fitting_space(layout.dimensions);
        }

        // Change the usable space and size of the space.
        self.update_metrics(layout.dimensions.generalized(self.ctx.axes));

        // Add the box to the vector and remember that spacings are allowed
        // again.
        self.space.layouts.push((self.ctx.axes, layout));
        self.space.last_spacing = LastSpacing::None;
    }

    /// Add multiple layouts to the stack.
    ///
    /// This function simply calls `add` repeatedly for each layout.
    pub fn add_multiple(&mut self, layouts: MultiLayout) {
        for layout in layouts {
            self.add(layout);
        }
    }

    /// Add secondary spacing to the stack.
    pub fn add_spacing(&mut self, mut spacing: Size, kind: SpacingKind) {
        match kind {
            // A hard space is simply an empty box.
            SpacingKind::Hard => {
                // Reduce the spacing such that it definitely fits.
                spacing.min_eq(self.space.usable.get_secondary(self.ctx.axes));
                let dimensions = Size2D::with_y(spacing);

                self.update_metrics(dimensions);
                self.space.layouts.push((self.ctx.axes, Layout {
                    dimensions: dimensions.specialized(self.ctx.axes),
                    alignment: LayoutAlignment::new(Origin, Origin),
                    actions: vec![]
                }));

                self.space.last_spacing = LastSpacing::Hard;
            }

            // A soft space is cached if it is not consumed by a hard space or
            // previous soft space with higher level.
            SpacingKind::Soft(level) => {
                let consumes = match self.space.last_spacing {
                    LastSpacing::None => true,
                    LastSpacing::Soft(_, prev) if level < prev => true,
                    _ => false,
                };

                if consumes {
                    self.space.last_spacing = LastSpacing::Soft(spacing, level);
                }
            }
        }
    }

    /// Update the size metrics to reflect that a layout or spacing with the
    /// given generalized dimensions has been added.
    fn update_metrics(&mut self, dimensions: Size2D) {
        let axes = self.ctx.axes;

        let mut size = self.space.size.generalized(axes);
        let mut extra = self.space.extra.generalized(axes);

        size.x += (dimensions.x - extra.x).max(Size::ZERO);
        size.y += (dimensions.y - extra.y).max(Size::ZERO);

        extra.x.max_eq(dimensions.x);
        extra.y = (extra.y - dimensions.y).max(Size::ZERO);

        self.space.size = size.specialized(axes);
        self.space.extra = extra.specialized(axes);
        *self.space.usable.get_secondary_mut(axes) -= dimensions.y;
    }

    /// Update the rulers to account for the new layout. Returns true if a
    /// space break is necessary.
    fn update_rulers(&mut self, alignment: LayoutAlignment) -> bool {
        let allowed = self.is_fitting_alignment(alignment);
        if allowed {
            *self.space.rulers.get_mut(self.ctx.axes.secondary, Origin)
                = alignment.secondary;
        }
        allowed
    }

    /// Whether a layout with the given alignment can still be layouted in the
    /// active space.
    pub fn is_fitting_alignment(&mut self, alignment: LayoutAlignment) -> bool {
        self.is_fitting_axis(self.ctx.axes.primary, alignment.primary)
            && self.is_fitting_axis(self.ctx.axes.secondary, alignment.secondary)
    }

    /// Whether the given alignment is still allowed according to the rulers.
    fn is_fitting_axis(&mut self, direction: Direction, alignment: Alignment) -> bool {
        alignment >= *self.space.rulers.get_mut(direction, Origin)
        && alignment <= self.space.rulers.get_mut(direction, End).inv()
    }

    /// Change the layouting axes used by this layouter.
    pub fn set_axes(&mut self, axes: LayoutAxes) {
        // Forget the spacing because it is not relevant anymore.
        if axes.secondary != self.ctx.axes.secondary {
            self.space.last_spacing = LastSpacing::Hard;
        }

        self.ctx.axes = axes;
    }

    /// Change the layouting spaces to use.
    ///
    /// If `replace_empty` is true, the current space is replaced if there are
    /// no boxes laid into it yet. Otherwise, only the followup spaces are
    /// replaced.
    pub fn set_spaces(&mut self, spaces: LayoutSpaces, replace_empty: bool) {
        if replace_empty && self.space_is_empty() {
            self.ctx.spaces = spaces;
            self.start_space(0, self.space.hard);
        } else {
            self.ctx.spaces.truncate(self.space.index + 1);
            self.ctx.spaces.extend(spaces);
        }
    }

    /// Move to the first space that can fit the given dimensions or do nothing
    /// if no space is capable of that.
    pub fn skip_to_fitting_space(&mut self, dimensions: Size2D) {
        let start = self.next_space();
        for (index, space) in self.ctx.spaces[start..].iter().enumerate() {
            if space.usable().fits(dimensions) {
                self.finish_space(true);
                self.start_space(start + index, true);
                return;
            }
        }
    }

    /// The remaining unpadded, unexpanding spaces. If a multi-layout is laid
    /// out into these spaces, it will fit into this stack.
    pub fn remaining(&self) -> LayoutSpaces {
        let dimensions = self.usable();

        let mut spaces = smallvec![LayoutSpace {
            dimensions,
            padding: SizeBox::ZERO,
            expansion: LayoutExpansion::new(false, false),
        }];

        for space in &self.ctx.spaces[self.next_space()..] {
            spaces.push(space.usable_space());
        }

        spaces
    }

    /// The remaining usable size.
    pub fn usable(&self) -> Size2D {
        self.space.usable
            - Size2D::with_y(self.space.last_spacing.soft_or_zero())
                .specialized(self.ctx.axes)
    }

    /// Whether the current layout space (not subspace) is empty.
    pub fn space_is_empty(&self) -> bool {
        self.space.size == Size2D::ZERO && self.space.layouts.is_empty()
    }

    /// Whether the current layout space is the last is the followup list.
    pub fn space_is_last(&self) -> bool {
        self.space.index == self.ctx.spaces.len() - 1
    }

    /// Compute the finished multi-layout.
    pub fn finish(mut self) -> MultiLayout {
        if self.space.hard || !self.space_is_empty() {
            self.finish_space(false);
        }
        self.layouts
    }

    /// Finish the current space and start a new one.
    pub fn finish_space(&mut self, hard: bool) {
        let space = self.ctx.spaces[self.space.index];

        // ------------------------------------------------------------------ //
        // Step 1: Determine the full dimensions of the space.
        // (Mostly done already while collecting the boxes, but here we
        //  expand if necessary.)

        let usable = space.usable();
        if space.expansion.horizontal { self.space.size.x = usable.x; }
        if space.expansion.vertical   { self.space.size.y = usable.y; }

        let dimensions = self.space.size.padded(space.padding);

        // ------------------------------------------------------------------ //
        // Step 2: Forward pass. Create a bounding box for each layout in which
        // it will be aligned. Then, go forwards through the boxes and remove
        // what is taken by previous layouts from the following layouts.

        let start = space.start();

        let mut bounds = vec![];
        let mut bound = SizeBox {
            left: start.x,
            top: start.y,
            right: start.x + self.space.size.x,
            bottom: start.y + self.space.size.y,
        };

        for (axes, layout) in &self.space.layouts {
            // First, we store the bounds calculated so far (which were reduced
            // by the predecessors of this layout) as the initial bounding box
            // of this layout.
            bounds.push(bound);

            // Then, we reduce the bounding box for the following layouts. This
            // layout uses up space from the origin to the end. Thus, it reduces
            // the usable space for following layouts at it's origin by its
            // extent along the secondary axis.
            *bound.get_mut(axes.secondary, Origin)
                += axes.secondary.factor() * layout.dimensions.get_secondary(*axes);
        }

        // ------------------------------------------------------------------ //
        // Step 3: Backward pass. Reduce the bounding boxes from the previous
        // layouts by what is taken by the following ones.

        // The `x` field stores the maximal primary extent in one axis-aligned
        // run, while the `y` fields stores the accumulated secondary extent.
        let mut extent = Size2D::ZERO;
        let mut rotation = Vertical;

        for (bound, entry) in bounds.iter_mut().zip(&self.space.layouts).rev() {
            let (axes, layout) = entry;

            // When the axes get rotated, the the maximal primary size
            // (`extent.x`) dictates how much secondary extent the whole run
            // had. This value is thus stored in `extent.y`. The primary extent
            // is reset for this new axis-aligned run.
            if rotation != axes.secondary.axis() {
                extent.y = extent.x;
                extent.x = Size::ZERO;
                rotation = axes.secondary.axis();
            }

            // We reduce the bounding box of this layout at it's end by the
            // accumulated secondary extent of all layouts we have seen so far,
            // which are the layouts after this one since we iterate reversed.
            *bound.get_mut(axes.secondary, End)
                -= axes.secondary.factor() * extent.y;

            // Then, we add this layout's secondary extent to the accumulator.
            let size = layout.dimensions.generalized(*axes);
            extent.x.max_eq(size.x);
            extent.y += size.y;
        }

        // ------------------------------------------------------------------ //
        // Step 4: Align each layout in its bounding box and collect everything
        // into a single finished layout.

        let mut actions = LayoutActions::new();

        if self.ctx.debug {
            actions.add(LayoutAction::DebugBox(dimensions));
        }

        let layouts = std::mem::replace(&mut self.space.layouts, vec![]);
        for ((axes, layout), bound) in layouts.into_iter().zip(bounds) {
            let size = layout.dimensions.specialized(axes);
            let alignment = layout.alignment;

            // The space in which this layout is aligned is given by the
            // distances between the borders of it's bounding box.
            let usable =
                Size2D::new(bound.right - bound.left, bound.bottom - bound.top)
                    .generalized(axes);

            let local = usable.anchor(alignment, axes) - size.anchor(alignment, axes);
            let pos = Size2D::new(bound.left, bound.top) + local.specialized(axes);

            actions.add_layout(pos, layout);
        }

        self.layouts.push(Layout {
            dimensions,
            alignment: self.ctx.alignment,
            actions: actions.to_vec(),
        });

        // ------------------------------------------------------------------ //
        // Step 5: Start the next space.

        self.start_space(self.next_space(), hard)
    }

    /// Start a new space with the given index.
    fn start_space(&mut self, index: usize, hard: bool) {
        let space = self.ctx.spaces[index];
        self.space = Space::new(index, hard, space.usable());
    }

    /// The index of the next space.
    fn next_space(&self) -> usize {
        (self.space.index + 1).min(self.ctx.spaces.len() - 1)
    }
}

impl Space {
    fn new(index: usize, hard: bool, usable: Size2D) -> Space {
        Space {
            index,
            hard,
            layouts: vec![],
            size: Size2D::ZERO,
            usable,
            extra: Size2D::ZERO,
            rulers: ValueBox::with_all(Origin),
            last_spacing: LastSpacing::Hard,
        }
    }
}
