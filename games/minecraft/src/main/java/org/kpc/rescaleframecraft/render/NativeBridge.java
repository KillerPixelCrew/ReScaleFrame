package org.kpc.rescaleframecraft.render;

import java.nio.FloatBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;
import net.minecraft.client.Minecraft;

/** Graphics calls stay on the render thread. Images remain on the GPU; JNI carries metadata and handles. */
final class NativeBridge {
    static final int FRAME_BEGIN=0,INPUT_SAMPLE=1,SIMULATION_END=3,PACING=6;
    private static Path loadedRoot;
    static Path load() throws Exception {
        if(loadedRoot!=null)return loadedRoot;
        byte[] manifest;
        try(var input=NativeBridge.class.getResourceAsStream("/native/windows-x64/files.sha256")) {
            if(input==null)throw new IllegalStateException("Native runtime is missing from this build");
            manifest=input.readAllBytes();
        }
        var sha=MessageDigest.getInstance("SHA-256");
        var hex=HexFormat.of();
        Path root=Minecraft.getInstance().gameDirectory.toPath().resolve("rescaleframe/native/")
            .resolve(hex.formatHex(sha.digest(manifest)).substring(0,20)).toAbsolutePath().normalize();
        for(String entry:new String(manifest,StandardCharsets.UTF_8).lines().toList()) {
            if(entry.isBlank())continue;
            String[] parts=entry.split("  ",2);
            if(parts.length!=2)throw new IllegalStateException("Invalid native manifest");
            Path file=root.resolve(parts[1]).normalize();
            if(!file.startsWith(root))throw new IllegalStateException("Invalid native resource path");
            if(Files.isRegularFile(file)&&hex.formatHex(sha.digest(Files.readAllBytes(file))).equalsIgnoreCase(parts[0]))continue;
            byte[] bytes;
            try(var input=NativeBridge.class.getResourceAsStream("/native/windows-x64/"+parts[1])) {
                if(input==null)throw new IllegalStateException("Missing native file: "+parts[1]);bytes=input.readAllBytes();
            }
            if(!hex.formatHex(sha.digest(bytes)).equalsIgnoreCase(parts[0]))throw new IllegalStateException("Native checksum mismatch: "+parts[1]);
            Files.createDirectories(file.getParent());Files.write(file,bytes);
        }
        System.load(root.resolve("ReScaleFrame.Minecraft.dll").toString());
        return loadedRoot=root;
    }
    static native long createSurface(long parent);
    static native void input(long pointer,long frame,int kinds);
    static native void destroySurface(long window);
    static native long open(long hwnd,long surface,String root);
    static native int[] configure(long handle,int width,int height,int sr,int quality,int preset,int fg,int count,int reflex,int limit,boolean debug);
    static native void cpu(long handle,long frame,int stage);
    static native void abort(long handle,long frame);
    static native int evaluate(long handle,long frame,FloatBuffer camera,boolean reset,int screen);
    static native int hudless(long handle,long frame,FloatBuffer camera);
    static native int present(long handle,long frame,boolean vsync);
    static native void status(long handle,long[] destination);
    static native int close(long handle);
}
