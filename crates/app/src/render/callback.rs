//! The egui_wgpu paint callback that hands one frame's [`CanvasFrame`] to the [`CanvasRenderer`]
//! stored in eframe's callback resources.

use eframe::egui;

use crate::render::gpu::{CanvasFrame, CanvasRenderer};

pub struct CanvasCallback {
    pub frame: CanvasFrame,
}

impl egui_wgpu::CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let renderer: &mut CanvasRenderer = callback_resources
            .get_mut()
            .expect("CanvasRenderer is installed by callback::install before the first frame");
        renderer.prepare(device, queue, &self.frame)
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let renderer: &CanvasRenderer = callback_resources
            .get()
            .expect("CanvasRenderer is installed by callback::install before the first frame");
        renderer.paint(render_pass);
    }
}

/// Inserts a [`CanvasRenderer`] into eframe's callback resources so [`CanvasCallback`] can find
/// it each frame; called once from `NapkinApp::new` when `cc.wgpu_render_state` is `Some`.
pub fn install(render_state: &egui_wgpu::RenderState) {
    let renderer = CanvasRenderer::new(
        &render_state.device,
        &render_state.queue,
        render_state.target_format,
    );
    render_state
        .renderer
        .write()
        .callback_resources
        .insert(renderer);
}
