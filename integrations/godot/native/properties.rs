// SPDX-License-Identifier: MIT
//! The scripting status schema's writer: each field is a typed `Property` handed to the consumer
//! (the GDExtension's Dictionary sink). Matrices and text are borrowed for the call.
use std::ffi::CStr;

pub(crate) enum Property<'a> {
    Integer(i64),
    Bool(bool),
    Real(f64),
    Text(&'a CStr),
    Extent([i32; 2]),
    /// Basis rows then the origin.
    Transform(&'a [f64; 12]),
    /// Columns.
    Projection(&'a [f64; 16]),
    /// The RenderingServer's current rendering method, read by the consumer.
    Renderer,
    /// The RenderingServer's current driver name, read by the consumer.
    Driver,
}

pub(crate) struct Properties<'a> {
    emit: &'a mut dyn FnMut(&CStr, Property),
}

impl<'a> Properties<'a> {
    pub(crate) fn new(emit: &'a mut dyn FnMut(&CStr, Property)) -> Self {
        Self { emit }
    }

    pub(crate) fn integer(&mut self, name: &CStr, integer: i64) {
        (self.emit)(name, Property::Integer(integer));
    }

    pub(crate) fn boolean(&mut self, name: &CStr, value: bool) {
        (self.emit)(name, Property::Bool(value));
    }

    pub(crate) fn real(&mut self, name: &CStr, real: f64) {
        (self.emit)(name, Property::Real(real));
    }

    pub(crate) fn text(&mut self, name: &CStr, text: &CStr) {
        (self.emit)(name, Property::Text(text));
    }

    pub(crate) fn extent(&mut self, name: &CStr, extent: [i32; 2]) {
        (self.emit)(name, Property::Extent(extent));
    }

    pub(crate) fn transform(&mut self, name: &CStr, matrix: &[f64; 12]) {
        (self.emit)(name, Property::Transform(matrix));
    }

    pub(crate) fn projection(&mut self, name: &CStr, matrix: &[f64; 16]) {
        (self.emit)(name, Property::Projection(matrix));
    }

    pub(crate) fn renderer(&mut self, name: &CStr) {
        (self.emit)(name, Property::Renderer);
    }

    pub(crate) fn driver(&mut self, name: &CStr) {
        (self.emit)(name, Property::Driver);
    }
}
