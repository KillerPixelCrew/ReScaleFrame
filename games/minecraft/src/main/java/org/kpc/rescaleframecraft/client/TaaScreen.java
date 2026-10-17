package org.kpc.rescaleframecraft.client;

import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.network.chat.Component;
import org.kpc.rescaleframecraft.render.TemporalRenderer;

public final class TaaScreen extends Screen {
    private final Screen parent;
    public TaaScreen(Screen parent) { super(Component.literal("ReScaleFrameCraft"));this.parent=parent; }
    @Override protected void init() {
        addRenderableWidget(Button.builder(label(),button->{
            TaaSettings.current.enabled=!TaaSettings.current.enabled;TaaSettings.save();button.setMessage(label());
        }).bounds(width/2-110,height/2-25,220,20).build());
        addRenderableWidget(Button.builder(Component.literal("Reset temporal history"),button->TemporalRenderer.reset())
            .bounds(width/2-110,height/2,220,20).build());
        addRenderableWidget(Button.builder(Component.translatable("gui.done"),button->onClose())
            .bounds(width/2-110,height/2+50,220,20).build());
    }
    private Component label() { return Component.literal("Temporal antialiasing: "+(TaaSettings.current.enabled?"On":"Off")); }
    @Override public void render(GuiGraphics graphics,int mouseX,int mouseY,float partial) {
        super.render(graphics,mouseX,mouseY,partial);
        graphics.drawCenteredString(font,title,width/2,height/2-70,0xffffff);
        graphics.drawCenteredString(font,TemporalRenderer.status(),width/2,height/2+30,0xb0b0b0);
    }
    @Override public void onClose() { minecraft.setScreen(parent); }
}
