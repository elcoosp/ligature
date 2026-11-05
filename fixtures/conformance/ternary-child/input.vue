<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Flip" });

const on = ref(false);
</script>

<template>
  <div>
    <b v-if="on">yes</b>
    <i v-else>no</i>
  </div>
</template>
