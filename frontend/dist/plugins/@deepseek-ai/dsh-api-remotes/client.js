window.__ModuleLoader__.load({
	id: "@deepseek-ai/dsh-api-remotes",
	factory: (require) => {
		var module = { exports: {} };
		var exports = module.exports;
		Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
		//#region \0rolldown/runtime.js
		var __create = Object.create;
		var __defProp = Object.defineProperty;
		var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
		var __getOwnPropNames = Object.getOwnPropertyNames;
		var __getProtoOf = Object.getPrototypeOf;
		var __hasOwnProp = Object.prototype.hasOwnProperty;
		var __copyProps = (to, from, except, desc) => {
			if (from && typeof from === "object" || typeof from === "function") for (var keys = __getOwnPropNames(from), i = 0, n = keys.length, key; i < n; i++) {
				key = keys[i];
				if (!__hasOwnProp.call(to, key) && key !== except) __defProp(to, key, {
					get: ((k) => from[k]).bind(null, key),
					enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable
				});
			}
			return to;
		};
		var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", {
			value: mod,
			enumerable: true
		}) : target, mod));
		//#endregion
		let _deepseek_ai_dsh_commands_remote = require("@deepseek-ai/dsh-commands/remote");
		_deepseek_ai_dsh_commands_remote = __toESM(_deepseek_ai_dsh_commands_remote, 1);
		let _deepseek_ai_dsh_goal_remote = require("@deepseek-ai/dsh-goal/remote");
		_deepseek_ai_dsh_goal_remote = __toESM(_deepseek_ai_dsh_goal_remote, 1);
		let _deepseek_ai_dsh_cordis_host_runner_remote = require("@deepseek-ai/dsh-cordis-host-runner/remote");
		_deepseek_ai_dsh_cordis_host_runner_remote = __toESM(_deepseek_ai_dsh_cordis_host_runner_remote, 1);
		let _deepseek_ai_dsh_host_plugin_inventory_remote = require("@deepseek-ai/dsh-host-plugin-inventory/remote");
		_deepseek_ai_dsh_host_plugin_inventory_remote = __toESM(_deepseek_ai_dsh_host_plugin_inventory_remote, 1);
		let _deepseek_ai_dsh_message_feedback_remote = require("@deepseek-ai/dsh-message-feedback/remote");
		_deepseek_ai_dsh_message_feedback_remote = __toESM(_deepseek_ai_dsh_message_feedback_remote, 1);
		//#region src/client/index.ts
		/** Required service: the typed Client Remote contribution mount. */
		const inject = ["remote"];
		/**
		* Mount the Host capabilities explicitly selected for this Client assembly.
		* @param ctx - Client Cordis root carrying the typed API service.
		* @returns disposer after every selected Remote namespace is ready.
		*/
		async function apply(ctx) {
			const disposers = [];
			try {
				for (const contribution of [
					_deepseek_ai_dsh_commands_remote.default,
					_deepseek_ai_dsh_goal_remote.default,
					_deepseek_ai_dsh_cordis_host_runner_remote.default,
					_deepseek_ai_dsh_host_plugin_inventory_remote.default,
					_deepseek_ai_dsh_message_feedback_remote.default
				]) disposers.push(await ctx.remote.$mount(contribution));
			} catch (error) {
				for (const dispose of disposers.reverse()) await dispose();
				throw error;
			}
			return async () => {
				for (const dispose of disposers.reverse()) await dispose();
			};
		}
		//#endregion
		exports.apply = apply;
		exports.inject = inject;
		return module.exports;
	}
});

//# sourceMappingURL=client.js.map