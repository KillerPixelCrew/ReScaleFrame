package org.kpc.rescaleframecraft.client;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import java.nio.file.Files;
import java.nio.file.Path;
import net.neoforged.fml.loading.FMLPaths;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.kpc.rescaleframecraft.render.TemporalRenderer;

public final class TaaSettings {
    private static final Gson GSON=new GsonBuilder().setPrettyPrinting().create();
    private static final Path FILE=FMLPaths.CONFIGDIR.get().resolve("rescaleframecraft.json");
    public static TaaSettings current=new TaaSettings();
    public boolean enabled=true;
    public enum Upscaler {
        OFF(0),AUTO(7),DLSS(1),FSR_2(2),FSR_3(3),FSR_4(4),XESS(5);
        public final int id;Upscaler(int id) {this.id=id;}
    }
    public enum Quality { NATIVE,QUALITY,BALANCED,PERFORMANCE,ULTRA_PERFORMANCE }
    public enum FrameGeneration {
        OFF(0),AUTO(7),DLSS(1),FSR_3(3),FSR_4(4),XESS(5);
        public final int id;FrameGeneration(int id) {this.id=id;}
    }
    public enum Reflex { OFF,ON,BOOST }
    public Upscaler upscaler=Upscaler.OFF;
    public Quality quality=Quality.QUALITY;
    public FrameGeneration frameGeneration=FrameGeneration.OFF;
    public Reflex reflex=Reflex.ON;
    public int generatedFrames=1,frameLimit=0;
    public boolean nativeDebug=false;
    public static void load() {
        if(Files.exists(FILE)) {
            try(var reader=Files.newBufferedReader(FILE)) {
                var loaded=GSON.fromJson(reader,TaaSettings.class);
                if(loaded!=null) {
                    current=loaded;
                    if(current.upscaler==null)current.upscaler=Upscaler.OFF;
                    if(current.quality==null)current.quality=Quality.QUALITY;
                    if(current.frameGeneration==null)current.frameGeneration=FrameGeneration.OFF;
                    if(current.reflex==null)current.reflex=Reflex.ON;
                    current.generatedFrames=Math.clamp(current.generatedFrames,1,15);
                    current.frameLimit=Math.clamp(current.frameLimit,0,240);
                }
            } catch(Exception e) { ReScaleFrameCraft.LOGGER.error("Unable to read TAA settings",e); }
        }
    }
    public static void save() {
        try { Files.createDirectories(FILE.getParent());Files.writeString(FILE,GSON.toJson(current)); }
        catch(Exception e) { ReScaleFrameCraft.LOGGER.error("Unable to save TAA settings",e); }
        TemporalRenderer.reset();
    }
}
