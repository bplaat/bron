; Transforms the position by the model view and projection matrices like GX, passes the texture
; coordinate on, mixes the day and night colors and finds the fog of the distance from the eye

; $MODE = "UniformRegister"
; $SPI_VS_OUT_CONFIG.VS_EXPORT_COUNT = 2
; $NUM_SPI_VS_OUT_ID = 1
; $SPI_VS_OUT_ID[0].SEMANTIC_0 = 0
; $SPI_VS_OUT_ID[0].SEMANTIC_1 = 1
; $SPI_VS_OUT_ID[0].SEMANTIC_2 = 2

; C0 to C3, the rows of the projection matrix, like libogc's Mtx44
; $UNIFORM_VARS[0].name = "uProjection"
; $UNIFORM_VARS[0].type = "mat4"
; $UNIFORM_VARS[0].count = 1
; $UNIFORM_VARS[0].block = -1
; $UNIFORM_VARS[0].offset = 0
; C4 to C6, the rows of the model view matrix, like libogc's Mtx
; $UNIFORM_VARS[1].name = "uModelView"
; $UNIFORM_VARS[1].type = "mat3x4"
; $UNIFORM_VARS[1].count = 1
; $UNIFORM_VARS[1].block = -1
; $UNIFORM_VARS[1].offset = 16
; C7, how much of the night color shows, and the scale and offset of the fog by the eye z
; $UNIFORM_VARS[2].name = "uParams"
; $UNIFORM_VARS[2].type = "vec4"
; $UNIFORM_VARS[2].count = 1
; $UNIFORM_VARS[2].block = -1
; $UNIFORM_VARS[2].offset = 28

; R1
; $ATTRIB_VARS[0].name = "aPosition"
; $ATTRIB_VARS[0].type = "vec4"
; $ATTRIB_VARS[0].location = 0
; R2
; $ATTRIB_VARS[1].name = "aTexCoord"
; $ATTRIB_VARS[1].type = "vec2"
; $ATTRIB_VARS[1].location = 1
; R3
; $ATTRIB_VARS[2].name = "aColor"
; $ATTRIB_VARS[2].type = "vec4"
; $ATTRIB_VARS[2].location = 2
; R4
; $ATTRIB_VARS[3].name = "aNight"
; $ATTRIB_VARS[3].type = "vec4"
; $ATTRIB_VARS[3].location = 3

00 CALL_FS NO_BARRIER
01 ALU: ADDR(32) CNT(37)
    0  x: DOT4   R127.x, R1.x, C4.x
       y: DOT4   ____, R1.y, C4.y
       z: DOT4   ____, R1.z, C4.z
       w: DOT4   ____, R1.w, C4.w
    1  x: DOT4   ____, R1.x, C5.x
       y: DOT4   R127.y, R1.y, C5.y
       z: DOT4   ____, R1.z, C5.z
       w: DOT4   ____, R1.w, C5.w
    2  x: DOT4   ____, R1.x, C6.x
       y: DOT4   ____, R1.y, C6.y
       z: DOT4   R127.z, R1.z, C6.z
       w: DOT4   ____, R1.w, C6.w
    3  x: DOT4   R1.x, R127.x, C0.x
       y: DOT4   ____, R127.y, C0.y
       z: DOT4   ____, R127.z, C0.z
       w: DOT4   ____, R1.w, C0.w
    4  x: DOT4   ____, R127.x, C1.x
       y: DOT4   R1.y, R127.y, C1.y
       z: DOT4   ____, R127.z, C1.z
       w: DOT4   ____, R1.w, C1.w
    5  x: DOT4   ____, R127.x, C2.x
       y: DOT4   ____, R127.y, C2.y
       z: DOT4   R1.z, R127.z, C2.z
       w: DOT4   ____, R1.w, C2.w
    6  x: DOT4   ____, R127.x, C3.x
       y: DOT4   ____, R127.y, C3.y
       z: DOT4   ____, R127.z, C3.z
       w: DOT4   R1.w, R1.w, C3.w
    7  x: ADD    R4.x, R4.x, -R3.x
       y: ADD    R4.y, R4.y, -R3.y
       z: ADD    R4.z, R4.z, -R3.z
       w: ADD    R4.w, R4.w, -R3.w
    8  x: MULADD R3.x, R4.x, C7.x, R3.x
       y: MULADD R3.y, R4.y, C7.x, R3.y
       z: MULADD R3.z, R4.z, C7.x, R3.z
       w: MULADD R3.w, R4.w, C7.x, R3.w
    9  x: MULADD R4.x, R127.z, C7.y, C7.z CLAMP
02 EXP_DONE: POS0, R1
03 EXP: PARAM0, R2 NO_BARRIER
04 EXP: PARAM1, R3 NO_BARRIER
05 EXP_DONE: PARAM2, R4.xxxx NO_BARRIER
END_OF_PROGRAM
