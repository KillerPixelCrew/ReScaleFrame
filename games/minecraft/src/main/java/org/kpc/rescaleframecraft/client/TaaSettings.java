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
    private static Path file() {return FMLPaths.CONFIGDIR.get().resolve("rescaleframecraft.json");}
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
    public enum DlssPreset {
        DEFAULT(0),E(5),F(6),J(10),K(11),L(12),M(13);
        public final int id;DlssPreset(int id) {this.id=id;}
    }
    public DlssPreset dlssPreset=DlssPreset.DEFAULT;
    public enum Reflex { OFF,ON,BOOST }
    public Upscaler upscaler=Upscaler.OFF;
    public Quality quality=Quality.QUALITY;
    public FrameGeneration frameGeneration=FrameGeneration.OFF;
    public Reflex reflex=Reflex.ON;
    public int generatedFrames=1,frameLimit=0;
    public boolean nativeDebug=false;
    public enum Sharpener { OFF,RCAS,DEPTH_RCAS,DEPTH_DAS,VENDOR_FSR_ONLY }
    public Sharpener sharpeningMode=Sharpener.OFF;
    public int sharpening_sharpness=400;
    public boolean sharpening_mas=false;
    public int sharpening_motion_sharpness=200;
    public int sharpening_motion_threshold=0;
    public int sharpening_motion_scale_limit=10000;
    public boolean sharpening_contrast_enabled=false;
    public int sharpening_contrast=0;
    public int sharpening_depth_scale=35000;
    public int sharpening_depth_bias=1000;
    public boolean sharpening_clamp_output=false;
    public boolean sharpening_debug=false;
    public void sanitizeSharpening() {
        if(sharpeningMode==null)sharpeningMode=Sharpener.OFF;
        sharpening_sharpness=Math.clamp(sharpening_sharpness,0,1000);
        sharpening_motion_sharpness=Math.clamp(sharpening_motion_sharpness,-1000,1000);
        sharpening_motion_threshold=Math.clamp(sharpening_motion_threshold,0,100000);
        sharpening_motion_scale_limit=Math.clamp(sharpening_motion_scale_limit,10,100000);
        sharpening_contrast=Math.clamp(sharpening_contrast,0,2000);
        sharpening_depth_scale=Math.clamp(sharpening_depth_scale,0,600000);
        sharpening_depth_bias=Math.clamp(sharpening_depth_bias,0,1000000);
    }
    public float[] sharpening() { return new float[]{sharpeningMode.ordinal(),sharpening_sharpness/1000.0f,sharpening_mas?1f:0f,sharpening_motion_sharpness/1000.0f,sharpening_motion_threshold/1000.0f,sharpening_motion_scale_limit/1000.0f,sharpening_contrast_enabled?1f:0f,sharpening_contrast/1000.0f,sharpening_depth_scale/1000.0f,sharpening_depth_bias/1000000.0f,sharpening_clamp_output?1f:0f,sharpening_debug?1f:0f}; }
    public record SharpeningState(Sharpener mode,int sharpness,boolean mas,int motion_sharpness,int motion_threshold,int motion_scale_limit,boolean contrast_enabled,int contrast,int depth_scale,int depth_bias,boolean clamp_output,boolean debug) {
        public boolean matches(TaaSettings settings) { return mode==settings.sharpeningMode&&
            sharpness==settings.sharpening_sharpness&&mas==settings.sharpening_mas&&motion_sharpness==settings.sharpening_motion_sharpness&&motion_threshold==settings.sharpening_motion_threshold&&motion_scale_limit==settings.sharpening_motion_scale_limit&&contrast_enabled==settings.sharpening_contrast_enabled&&contrast==settings.sharpening_contrast&&depth_scale==settings.sharpening_depth_scale&&depth_bias==settings.sharpening_depth_bias&&clamp_output==settings.sharpening_clamp_output&&debug==settings.sharpening_debug; }
    }
    public SharpeningState sharpeningState() { return new SharpeningState(sharpeningMode,sharpening_sharpness,sharpening_mas,sharpening_motion_sharpness,sharpening_motion_threshold,sharpening_motion_scale_limit,sharpening_contrast_enabled,sharpening_contrast,sharpening_depth_scale,sharpening_depth_bias,sharpening_clamp_output,sharpening_debug); }
    public static void load() {
        Path FILE=file();
        if(Files.exists(FILE)) {
            try(var reader=Files.newBufferedReader(FILE)) {
                var loaded=GSON.fromJson(reader,TaaSettings.class);
                if(loaded!=null) {
                    current=loaded;
                    current.sanitizeSharpening();
                    if(current.upscaler==null)current.upscaler=Upscaler.OFF;
                    if(current.dlssPreset==null)current.dlssPreset=DlssPreset.DEFAULT;
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
        current.sanitizeSharpening();
        current.generatedFrames=FrameGenerationLimits.INSTANCE.clamp(current.frameGeneration,current.generatedFrames);
        Path FILE=file();
        try { Files.createDirectories(FILE.getParent());Files.writeString(FILE,GSON.toJson(current)); }
        catch(Exception e) { ReScaleFrameCraft.LOGGER.error("Unable to save TAA settings",e); }
        TemporalRenderer.reset();
    }
}
