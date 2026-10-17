package org.kpc.rescaleframecraft.render.flywheel;

import java.lang.reflect.Method;

/** Optional Iris public API; no shader-pack or Iris classes are required without that mod. */
public final class IrisState {
    private static final Object api;
    private static final Method shadow,pack;
    static {
        Object instance=null;Method method=null,packMethod=null;
        try {
            var type=Class.forName("net.irisshaders.iris.api.v0.IrisApi");
            instance=type.getMethod("getInstance").invoke(null);method=type.getMethod("isRenderingShadowPass");
            packMethod=type.getMethod("isShaderPackInUse");
        } catch(ReflectiveOperationException absent) {instance=null;method=null;packMethod=null;}
        api=instance;shadow=method;pack=packMethod;
    }
    public static boolean shadowPass() {
        if(api==null)return false;
        try {return (Boolean)shadow.invoke(api);}
        catch(ReflectiveOperationException failure) {throw new IllegalStateException("Iris shadow state unavailable",failure);}
    }
    public static boolean packActive() {
        if(api==null)return false;
        try {return (Boolean)pack.invoke(api);}
        catch(ReflectiveOperationException failure) {throw new IllegalStateException("Iris pack state unavailable",failure);}
    }
}
